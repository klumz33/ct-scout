// src/platforms/intigriti.rs
//! Intigriti API integration for automatic watchlist synchronization

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION};
use serde_json::Value;
use tracing::{debug, info, warn};

use super::{extract_domain, FetchOptions, PlatformAPI, Program};

/// Intigriti API client
pub struct IntigritiAPI {
    api_token: String,
    client: reqwest::Client,
    base_url: String,
    filter: String,
    max_programs: usize,
}

impl IntigritiAPI {
    /// Create new Intigriti API client
    pub fn new(api_token: String, filter: String, max_programs: usize) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            api_token,
            client,
            base_url: "https://api.intigriti.com/external/researcher".to_string(),
            filter,
            max_programs,
        })
    }

    /// Fetch programs list with pagination and filtering
    async fn fetch_programs_list_paginated(&self, filter: &str, max_programs: usize) -> Result<Vec<Value>> {
        info!("Fetching programs from Intigriti (filter: {}, max: {})", filter, max_programs);

        let mut all_programs = Vec::new();
        let mut offset = 0;
        let limit = 500; // Maximum allowed by Intigriti API

        loop {
            // Build URL with pagination and following filter
            let mut url = format!(
                "{}/v1/programs?limit={}&offset={}",
                self.base_url, limit, offset
            );

            // Add following filter if requested
            if filter == "following" {
                url.push_str("&following=true");
            }

            debug!("Fetching Intigriti offset {} (limit: {}, filter: {})", offset, limit, filter);

            let response = self
                .client
                .get(&url)
                .header(
                    AUTHORIZATION,
                    format!("Bearer {}", self.api_token),
                )
                .send()
                .await
                .context("Failed to send request to Intigriti API")?;

            if !response.status().is_success() {
                anyhow::bail!(
                    "Intigriti API returned error: {} - {}",
                    response.status(),
                    response.text().await.unwrap_or_default()
                );
            }

            let json: Value = response
                .json()
                .await
                .context("Failed to parse Intigriti API response")?;

            let programs = json["records"]
                .as_array()
                .context("Invalid response format from Intigriti")?
                .clone();

            let max_count = json["maxCount"].as_u64().unwrap_or(0) as usize;

            if programs.is_empty() {
                debug!("No more programs at offset {}", offset);
                break;
            }

            debug!("Found {} programs at offset {} (total available: {})", programs.len(), offset, max_count);

            for program in programs {
                all_programs.push(program);
                if all_programs.len() >= max_programs {
                    info!("Reached max_programs limit of {}", max_programs);
                    return Ok(all_programs);
                }
            }

            // Check if we've fetched all available programs
            if all_programs.len() >= max_count {
                debug!("Fetched all {} available programs", max_count);
                break;
            }

            offset += limit;
        }

        info!("Found {} total programs on Intigriti (filter: {})", all_programs.len(), filter);
        Ok(all_programs)
    }

    /// Fetch program details including scope
    async fn fetch_program_details(&self, program_id: &str) -> Result<Vec<String>> {
        debug!("Fetching scope for program: {}", program_id);

        let url = format!("{}/v1/programs/{}", self.base_url, program_id);

        let response = self
            .client
            .get(&url)
            .header(
                AUTHORIZATION,
                format!("Bearer {}", self.api_token),
            )
            .send()
            .await
            .context("Failed to fetch program details")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();

            // Parse error code if it's JSON
            let error_code = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&error_body) {
                json["code"].as_str().unwrap_or("UNKNOWN").to_string()
            } else {
                "UNKNOWN".to_string()
            };

            // FORBID001 is expected for programs you're not enrolled in or restricted programs
            // Only log at debug level to reduce noise
            if status.as_u16() == 403 && error_code == "FORBID001" {
                debug!(
                    "Program {} is restricted or not accessible (403 FORBID001)",
                    program_id
                );
            } else {
                // Other errors are unexpected and should be warned
                warn!(
                    "Failed to fetch scope for program {}: HTTP {} - {}",
                    program_id,
                    status,
                    if error_body.is_empty() { "no error message" } else { &error_body }
                );
            }

            return Ok(Vec::new());
        }

        let json: Value = response
            .json()
            .await
            .context("Failed to parse program details")?;

        let mut domains = Vec::new();

        // Extract domains from program scope
        // API v1.0 structure: response.domains.content[] with each having endpoint, type, tier
        if let Some(domains_obj) = json.get("domains") {
            if let Some(content_array) = domains_obj.get("content").and_then(|v| v.as_array()) {
                for domain_obj in content_array {
                    // Check if domain is in scope via tier
                    // tier is an object: { id: number, value: string }
                    // Example: tier.id: 3 = "Tier 2", tier.id: 4 = "Tier 1", tier.id: 5 = "Out Of Scope"
                    let tier_id = domain_obj
                        .get("tier")
                        .and_then(|t| t.get("id"))
                        .and_then(|id| id.as_i64())
                        .unwrap_or(0);

                    let tier_value = domain_obj
                        .get("tier")
                        .and_then(|t| t.get("value"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    // Only include in-scope domains (tier.id < 5, i.e., not "Out Of Scope")
                    // Tier 1-4 are all in-scope, Tier 5 is out of scope
                    if tier_id > 0 && tier_id < 5 && tier_value != "Out Of Scope" {
                        let domain_type = domain_obj
                            .get("type")
                            .and_then(|t| t.get("value"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        let endpoint = domain_obj
                            .get("endpoint")
                            .and_then(|e| e.as_str())
                            .unwrap_or("");

                        // Extract domains from url and wildcard types
                        // Type values are capitalized: "Url", "Wildcard"
                        if (domain_type.eq_ignore_ascii_case("url") || domain_type.eq_ignore_ascii_case("wildcard"))
                            && !endpoint.is_empty() {
                            let domain = extract_domain(endpoint);
                            if !domain.is_empty() {
                                domains.push(domain);
                            }
                        }
                    }
                }
            }
        }

        debug!(
            "Found {} domains for program: {}",
            domains.len(),
            program_id
        );
        Ok(domains)
    }
}

#[async_trait]
impl PlatformAPI for IntigritiAPI {
    fn name(&self) -> &str {
        "Intigriti"
    }

    async fn fetch_programs(&self) -> Result<Vec<Program>> {
        self.fetch_programs_with_options(FetchOptions {
            filter: self.filter.clone(),
            max_programs: self.max_programs,
            dry_run: false,
        }).await
    }

    async fn fetch_programs_with_options(&self, options: FetchOptions) -> Result<Vec<Program>> {
        let programs_list = self.fetch_programs_list_paginated(&options.filter, options.max_programs).await?;
        let total_programs = programs_list.len();
        let mut programs = Vec::new();
        let mut restricted_count = 0;
        let mut empty_scope_count = 0;

        info!(
            "Intigriti: {} programs to process (filter: {})",
            total_programs, options.filter
        );

        if options.dry_run {
            info!("DRY-RUN MODE: Showing programs that would be synced");
            info!("─────────────────────────────────────────────────────────────");

            for program_data in &programs_list {
                let name = program_data["name"].as_str().unwrap_or("").to_string();
                let handle = program_data["handle"].as_str().unwrap_or("").to_string();
                let following = program_data["following"].as_bool().unwrap_or(false);

                info!("Would sync: '{}' ({}) [following: {}]", name, handle, following);
            }

            info!("─────────────────────────────────────────────────────────────");
            info!("DRY-RUN: Would attempt to fetch scope for {} programs", total_programs);
            return Ok(Vec::new());
        }

        info!("Fetching scope details for each program...");
        info!("Note: 403 FORBID001 errors are expected for programs you're not enrolled in or that are restricted");

        for program_data in programs_list {
            let program_id = program_data["id"].as_str().unwrap_or("").to_string();
            let name = program_data["name"].as_str().unwrap_or("").to_string();
            let handle = program_data["handle"].as_str().unwrap_or("").to_string();

            if program_id.is_empty() {
                continue;
            }

            // Fetch scope for this program
            let domains = match self.fetch_program_details(&program_id).await {
                Ok(d) => d,
                Err(e) => {
                    warn!("Failed to fetch scope for {}: {}", program_id, e);
                    restricted_count += 1;
                    continue;
                }
            };

            if !domains.is_empty() {
                info!(
                    "✓ Intigriti: Program '{}' ({}): {} domains in scope",
                    name,
                    handle,
                    domains.len()
                );
                debug!("  Domains: {:?}", domains);
                programs.push(Program {
                    id: program_id.clone(),
                    name,
                    handle,
                    platform: "Intigriti".to_string(),
                    domains,
                    hosts: Vec::new(), // Intigriti API doesn't separate hosts
                    in_scope: true,
                });
            } else {
                empty_scope_count += 1;
            }
        }

        info!("─────────────────────────────────────────────────────────────");
        info!(
            "Intigriti sync complete: {} accessible programs with domains (out of {} total)",
            programs.len(),
            total_programs
        );
        info!("  • Accessible with domains: {}", programs.len());
        info!("  • Restricted/not enrolled: {}", restricted_count);
        info!("  • Empty scope: {}", empty_scope_count);
        info!("─────────────────────────────────────────────────────────────");
        Ok(programs)
    }

    async fn test_connection(&self) -> Result<bool> {
        let url = format!("{}/v1/programs", self.base_url);

        let response = self
            .client
            .get(&url)
            .header(
                AUTHORIZATION,
                format!("Bearer {}", self.api_token),
            )
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            warn!(
                "Intigriti API connection failed: HTTP {} - {}",
                status,
                if body.is_empty() { "no error message" } else { &body }
            );
            return Ok(false);
        }

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intigriti_api_creation() {
        let api = IntigritiAPI::new("test_token".to_string(), "following".to_string(), 100);
        assert!(api.is_ok());
    }
}
