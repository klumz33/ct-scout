// src/cert_parser.rs
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use x509_parser::extensions::ParsedExtension;
use x509_parser::prelude::*;

/// Parsed certificate with extracted metadata
#[derive(Debug, Clone)]
pub struct ParsedCert {
    pub domains: Vec<String>,
    pub not_before: Option<u64>,
    pub not_after: Option<u64>,
    pub fingerprint: String,
    pub issuer: Option<String>,
    pub is_precert: bool,
}

/// Certificate parser for extracting domains and metadata
pub struct CertificateParser;

impl CertificateParser {
    /// Parse base64-encoded DER certificate and extract all DNS names
    pub fn parse_domains(base64_der: &str) -> Result<Vec<String>> {
        let parsed = Self::parse_full(base64_der)?;
        Ok(parsed.domains)
    }

    /// Parse full certificate with all metadata
    pub fn parse_full(base64_der: &str) -> Result<ParsedCert> {
        // Decode base64
        use base64::Engine;
        let der_bytes = base64::engine::general_purpose::STANDARD.decode(base64_der)
            .context("Failed to decode base64 certificate")?;

        // Calculate SHA-256 fingerprint
        let fingerprint = {
            let mut hasher = Sha256::new();
            hasher.update(&der_bytes);
            hex::encode(hasher.finalize())
        };

        // Parse X.509 certificate
        let (_, cert) = X509Certificate::from_der(&der_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to parse X.509 certificate: {:?}", e))?;

        // Extract domains from Subject Alternative Name extension
        let mut domains = Vec::new();

        // Check SAN extension (OID 2.5.29.17)
        for ext in cert.extensions() {
            if let ParsedExtension::SubjectAlternativeName(san) = ext.parsed_extension() {
                for general_name in &san.general_names {
                    if let GeneralName::DNSName(dns_name) = general_name {
                        domains.push(dns_name.to_string());
                    }
                }
            }
        }

        // Fallback: Extract Common Name (CN) from subject if no SAN
        if domains.is_empty() {
            if let Some(cn) = Self::extract_cn(&cert) {
                domains.push(cn);
            }
        }

        // Extract validity period
        let not_before = Some(cert.validity().not_before.timestamp() as u64);
        let not_after = Some(cert.validity().not_after.timestamp() as u64);

        // Extract issuer
        let issuer = Self::extract_issuer(&cert);

        Ok(ParsedCert {
            domains,
            not_before,
            not_after,
            fingerprint,
            issuer,
            is_precert: false, // parse_full is for regular certs
        })
    }

    /// Extract Common Name (CN) from certificate subject
    fn extract_cn(cert: &X509Certificate) -> Option<String> {
        for rdn in cert.subject().iter() {
            for attr in rdn.iter() {
                if attr.attr_type() == &oid_registry::OID_X509_COMMON_NAME {
                    if let Ok(cn) = attr.attr_value().as_str() {
                        return Some(cn.to_string());
                    }
                }
            }
        }
        None
    }

    /// Extract issuer from certificate
    fn extract_issuer(cert: &X509Certificate) -> Option<String> {
        // Try to get CN from issuer
        for rdn in cert.issuer().iter() {
            for attr in rdn.iter() {
                if attr.attr_type() == &oid_registry::OID_X509_COMMON_NAME {
                    if let Ok(cn) = attr.attr_value().as_str() {
                        return Some(cn.to_string());
                    }
                }
            }
        }

        // Fallback: return full issuer DN as string
        Some(cert.issuer().to_string())
    }

    /// Parse CT log entry (handles both x509_entry and precert_entry types)
    /// For precerts, uses extra_data which contains full certificate (more reliable than TBSCertificate)
    /// Returns full certificate metadata including validity dates and fingerprint
    ///
    /// # Arguments
    /// * `base64_leaf_input` - The leaf_input field from CT log entry
    /// * `base64_extra_data` - The extra_data field from CT log entry
    /// * `parse_precerts` - Whether to parse precertificates (type 1 entries)
    pub fn parse_log_entry(base64_leaf_input: &str, base64_extra_data: &str, parse_precerts: bool) -> Result<ParsedCert> {
        use base64::Engine;

        // Decode leaf_input to check entry type
        let leaf_bytes = base64::engine::general_purpose::STANDARD.decode(base64_leaf_input)
            .context("Failed to decode base64 leaf_input")?;

        if leaf_bytes.len() < 12 {
            anyhow::bail!("Leaf input too short: {} bytes", leaf_bytes.len());
        }

        // Check entry type at bytes 10-11 (big-endian u16)
        let entry_type = ((leaf_bytes[10] as u16) << 8) | (leaf_bytes[11] as u16);

        match entry_type {
            0 => {
                // x509_entry: Certificate is in leaf_input
                if leaf_bytes.len() < 15 {
                    anyhow::bail!("x509_entry too short");
                }

                let cert_len = ((leaf_bytes[12] as usize) << 16)
                    | ((leaf_bytes[13] as usize) << 8)
                    | (leaf_bytes[14] as usize);

                let end_pos = std::cmp::min(15 + cert_len, leaf_bytes.len());
                let cert_der = &leaf_bytes[15..end_pos];

                Self::extract_full_cert_from_der(cert_der, false)
            }
            1 => {
                // precert_entry: Skip if precert parsing is disabled
                if !parse_precerts {
                    anyhow::bail!("Precertificate parsing disabled");
                }

                // Parse from extra_data (contains full precertificate)
                // extra_data format: 3-byte length + full X.509 precert + chain

                let extra_bytes = base64::engine::general_purpose::STANDARD.decode(base64_extra_data)
                    .context("Failed to decode base64 extra_data")?;

                if extra_bytes.len() < 3 {
                    anyhow::bail!("extra_data too short for precert_entry");
                }

                // Read 3-byte big-endian length
                let precert_len = ((extra_bytes[0] as usize) << 16)
                    | ((extra_bytes[1] as usize) << 8)
                    | (extra_bytes[2] as usize);

                if extra_bytes.len() < 3 + precert_len {
                    anyhow::bail!("extra_data truncated: expected {} bytes", 3 + precert_len);
                }

                // Extract precertificate DER (full X.509 certificate with poison extension)
                let precert_der = &extra_bytes[3..3 + precert_len];

                Self::extract_full_cert_from_der(precert_der, true)
            }
            _ => {
                anyhow::bail!("Unknown entry type: {}", entry_type);
            }
        }
    }

    /// Legacy function for backward compatibility - parses with precerts enabled by default
    pub fn parse_leaf_input(base64_leaf_input: &str) -> Result<Vec<String>> {
        let parsed = Self::parse_log_entry(base64_leaf_input, "", true)?;
        Ok(parsed.domains)
    }

    /// Extract full certificate metadata from DER-encoded certificate
    fn extract_full_cert_from_der(der_bytes: &[u8], is_precert: bool) -> Result<ParsedCert> {
        // Calculate SHA-256 fingerprint
        let fingerprint = {
            let mut hasher = Sha256::new();
            hasher.update(der_bytes);
            hex::encode(hasher.finalize())
        };

        // Parse X.509 certificate
        let (_, cert) = X509Certificate::from_der(der_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to parse certificate from DER: {:?}", e))?;

        // Extract domains from SAN
        let mut domains = Vec::new();

        for ext in cert.extensions() {
            if let ParsedExtension::SubjectAlternativeName(san) = ext.parsed_extension() {
                for general_name in &san.general_names {
                    if let GeneralName::DNSName(dns_name) = general_name {
                        domains.push(dns_name.to_string());
                    }
                }
            }
        }

        // Fallback to CN if no SAN
        if domains.is_empty() {
            if let Some(cn) = Self::extract_cn(&cert) {
                domains.push(cn);
            }
        }

        // Extract validity period
        let not_before = Some(cert.validity().not_before.timestamp() as u64);
        let not_after = Some(cert.validity().not_after.timestamp() as u64);

        // Extract issuer
        let issuer = Self::extract_issuer(&cert);

        Ok(ParsedCert {
            domains,
            not_before,
            not_after,
            fingerprint,
            issuer,
            is_precert,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_certificate() {
        // Invalid base64
        assert!(CertificateParser::parse_domains("invalid-base64").is_err());
    }

    #[test]
    fn test_parse_leaf_input_too_short() {
        // Base64 of just a few bytes
        use base64::Engine;
        let short_input = base64::engine::general_purpose::STANDARD.encode(b"short");
        assert!(CertificateParser::parse_leaf_input(&short_input).is_err());
    }
}
