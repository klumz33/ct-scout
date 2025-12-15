// src/platforms/intigriti.rs
//! Intigriti API integration for automatic watchlist synchronization

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION};
use serde_json::Value;
use tracing::{debug, info, warn};

use super::{extract_domain, PlatformAPI, Program};

/// Intigriti API client
pub struct IntigritiAPI {
    api_token: String,
    client: reqwest::Client,
    base_url: String,
}

impl IntigritiAPI {
    /// Create new Intigriti API client
    pub fn new(api_token: String) -> Result<Self> {
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
        })
    }

    /// Fetch programs list
    async fn fetch_programs_list(&self) -> Result<Vec<Value>> {
        info!("Fetching programs from Intigriti");

        let url = format!("{}/v1/programs", self.base_url);

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

        info!("Found {} programs on Intigriti", programs.len());
        Ok(programs)
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

            // Log the full error with response body for debugging
            warn!(
                "Failed to fetch scope for program {}: HTTP {} - {}",
                program_id,
                status,
                if error_body.is_empty() { "no error message" } else { &error_body }
            );

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
                    // tier.id: 1 = high, 2 = medium, 3 = low, 4 = out of scope
                    let tier_id = domain_obj
                        .get("tier")
                        .and_then(|t| t.get("id"))
                        .and_then(|id| id.as_i64())
                        .unwrap_or(0);

                    // Only include in-scope domains (tier 1-3)
                    if tier_id > 0 && tier_id < 4 {
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
                        if (domain_type == "url" || domain_type == "wildcard") && !endpoint.is_empty() {
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
        let programs_list = self.fetch_programs_list().await?;
        let mut programs = Vec::new();

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
                    continue;
                }
            };

            if !domains.is_empty() {
                programs.push(Program {
                    id: program_id.clone(),
                    name,
                    handle,
                    domains,
                    hosts: Vec::new(), // Intigriti API doesn't separate hosts
                    in_scope: true,
                });
            }
        }

        info!(
            "Successfully fetched {} programs from Intigriti",
            programs.len()
        );
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
        let api = IntigritiAPI::new("test_token".to_string());
        assert!(api.is_ok());
    }
}
