// src/platforms/hackerone.rs
//! HackerOne API integration for automatic watchlist synchronization

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT};
use serde_json::Value;
use tracing::{debug, info, warn};

use super::{extract_domain, PlatformAPI, Program};

/// HackerOne API client
pub struct HackerOneAPI {
    username: String,
    api_token: String,
    client: reqwest::Client,
    base_url: String,
}

impl HackerOneAPI {
    /// Create new HackerOne API client
    pub fn new(username: String, api_token: String) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            username,
            api_token,
            client,
            base_url: "https://api.hackerone.com".to_string(),
        })
    }

    /// Fetch programs list
    async fn fetch_programs_list(&self) -> Result<Vec<Value>> {
        info!("Fetching programs from HackerOne");

        let url = format!("{}/v1/hackers/programs", self.base_url);

        let response = self
            .client
            .get(&url)
            .basic_auth(&self.username, Some(&self.api_token))
            .send()
            .await
            .context("Failed to send request to HackerOne API")?;

        if !response.status().is_success() {
            anyhow::bail!(
                "HackerOne API returned error: {} - {}",
                response.status(),
                response.text().await.unwrap_or_default()
            );
        }

        let json: Value = response
            .json()
            .await
            .context("Failed to parse HackerOne API response")?;

        let programs = json["data"]
            .as_array()
            .context("Invalid response format from HackerOne")?
            .clone();

        info!("Found {} programs on HackerOne", programs.len());
        Ok(programs)
    }

    /// Fetch structured scope for a program
    async fn fetch_program_scope(&self, handle: &str) -> Result<Vec<String>> {
        debug!("Fetching scope for program: {}", handle);

        let url = format!("{}/v1/hackers/programs/{}", self.base_url, handle);

        let response = self
            .client
            .get(&url)
            .basic_auth(&self.username, Some(&self.api_token))
            .send()
            .await
            .context("Failed to fetch program details")?;

        if !response.status().is_success() {
            let status = response.status();

            // 403 Forbidden is expected for private/unenrolled programs
            // Only log at debug level to reduce noise
            if status.as_u16() == 403 {
                debug!(
                    "Program {} is restricted or not accessible (403 Forbidden)",
                    handle
                );
            } else {
                // Other errors are unexpected and should be warned
                warn!(
                    "Failed to fetch scope for {}: {}",
                    handle,
                    status
                );
            }
            return Ok(Vec::new());
        }

        let json: Value = response
            .json()
            .await
            .context("Failed to parse program details")?;

        let mut domains = Vec::new();

        if let Some(relationships) = json["data"]["relationships"].as_object() {
            if let Some(structured_scopes) = relationships.get("structured_scopes") {
                if let Some(scopes) = structured_scopes["data"].as_array() {
                    for scope in scopes {
                        // Only process in-scope items
                        if scope["attributes"]["eligible_for_submission"]
                            .as_bool()
                            .unwrap_or(false)
                        {
                            let asset_type = scope["attributes"]["asset_type"]
                                .as_str()
                                .unwrap_or("");
                            let asset_identifier = scope["attributes"]["asset_identifier"]
                                .as_str()
                                .unwrap_or("");

                            // Extract domains from URL and WILDCARD types
                            if (asset_type == "URL" || asset_type == "WILDCARD")
                                && !asset_identifier.is_empty()
                            {
                                let domain = extract_domain(asset_identifier);
                                if !domain.is_empty() {
                                    domains.push(domain);
                                }
                            }
                        }
                    }
                }
            }
        }

        debug!("Found {} domains for program: {}", domains.len(), handle);
        Ok(domains)
    }
}

#[async_trait]
impl PlatformAPI for HackerOneAPI {
    fn name(&self) -> &str {
        "HackerOne"
    }

    async fn fetch_programs(&self) -> Result<Vec<Program>> {
        let programs_list = self.fetch_programs_list().await?;
        let total_programs = programs_list.len();
        let mut programs = Vec::new();
        let mut restricted_count = 0;
        let mut empty_scope_count = 0;

        info!(
            "HackerOne API returned {} programs (programs you're invited to or public programs)",
            total_programs
        );
        info!("Fetching structured scope for each program...");
        info!("Note: 403 Forbidden errors are expected for private programs you're not enrolled in");

        for program_data in programs_list {
            let attributes = &program_data["attributes"];
            let handle = attributes["handle"].as_str().unwrap_or("").to_string();
            let name = attributes["name"].as_str().unwrap_or("").to_string();
            let id = program_data["id"].as_str().unwrap_or("").to_string();

            if handle.is_empty() {
                continue;
            }

            // Fetch scope for this program
            let domains = match self.fetch_program_scope(&handle).await {
                Ok(d) => d,
                Err(e) => {
                    warn!("Failed to fetch scope for {}: {}", handle, e);
                    restricted_count += 1;
                    continue;
                }
            };

            if !domains.is_empty() {
                info!(
                    "✓ Program '{}' (@{}): {} domains in scope",
                    name,
                    handle,
                    domains.len()
                );
                debug!("  Domains: {:?}", domains);
                programs.push(Program {
                    id,
                    name,
                    handle,
                    domains,
                    hosts: Vec::new(), // HackerOne API doesn't separate hosts
                    in_scope: true,
                });
            } else {
                empty_scope_count += 1;
            }
        }

        info!("─────────────────────────────────────────────────────────────");
        info!(
            "HackerOne sync complete: {} accessible programs with domains (out of {} total)",
            programs.len(),
            total_programs
        );
        info!("  • Accessible with domains: {}", programs.len());
        info!("  • Restricted/no access: {}", restricted_count);
        info!("  • Empty scope: {}", empty_scope_count);
        info!("─────────────────────────────────────────────────────────────");
        Ok(programs)
    }

    async fn test_connection(&self) -> Result<bool> {
        let url = format!("{}/v1/hackers/programs", self.base_url);

        let response = self
            .client
            .get(&url)
            .basic_auth(&self.username, Some(&self.api_token))
            .send()
            .await?;

        Ok(response.status().is_success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hackerone_api_creation() {
        let api = HackerOneAPI::new(
            "test_user".to_string(),
            "test_token".to_string(),
        );
        assert!(api.is_ok());
    }
}
