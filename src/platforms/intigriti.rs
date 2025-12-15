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
            base_url: "https://api.intigriti.com".to_string(),
        })
    }

    /// Fetch programs list
    async fn fetch_programs_list(&self) -> Result<Vec<Value>> {
        info!("Fetching programs from Intigriti");

        let url = format!("{}/core/researcher/programs", self.base_url);

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
    async fn fetch_program_details(&self, company_id: &str, program_id: &str) -> Result<Vec<String>> {
        debug!("Fetching scope for program: {}/{}", company_id, program_id);

        let url = format!(
            "{}/core/researcher/program/{}/{}",
            self.base_url, company_id, program_id
        );

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
            warn!(
                "Failed to fetch scope for {}/{}: {}",
                company_id,
                program_id,
                response.status()
            );
            return Ok(Vec::new());
        }

        let json: Value = response
            .json()
            .await
            .context("Failed to parse program details")?;

        let mut domains = Vec::new();

        // Extract domains from program scope
        if let Some(domains_array) = json["domains"].as_array() {
            for domain_obj in domains_array {
                // Check if domain is in scope
                let tier = domain_obj["tier"].as_i64().unwrap_or(0);

                // tier > 0 means in scope (1 = high, 2 = medium, 3 = low, 4 = out of scope)
                if tier > 0 && tier < 4 {
                    let domain_type = domain_obj["type"].as_str().unwrap_or("");
                    let content = domain_obj["content"].as_str().unwrap_or("");

                    // Extract domains from url and wildcard types
                    if (domain_type == "url" || domain_type == "wildcard") && !content.is_empty() {
                        let domain = extract_domain(content);
                        if !domain.is_empty() {
                            domains.push(domain);
                        }
                    }
                }
            }
        }

        debug!(
            "Found {} domains for program: {}/{}",
            domains.len(),
            company_id,
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
            let company_id = program_data["companyId"].as_str().unwrap_or("").to_string();
            let program_id = program_data["programId"].as_str().unwrap_or("").to_string();
            let name = program_data["name"].as_str().unwrap_or("").to_string();
            let handle = program_data["handle"].as_str().unwrap_or(&company_id).to_string();

            if company_id.is_empty() || program_id.is_empty() {
                continue;
            }

            // Fetch scope for this program
            let domains = match self.fetch_program_details(&company_id, &program_id).await {
                Ok(d) => d,
                Err(e) => {
                    warn!("Failed to fetch scope for {}/{}: {}", company_id, program_id, e);
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
        let url = format!("{}/core/researcher/programs", self.base_url);

        let response = self
            .client
            .get(&url)
            .header(
                AUTHORIZATION,
                format!("Bearer {}", self.api_token),
            )
            .send()
            .await?;

        Ok(response.status().is_success())
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
