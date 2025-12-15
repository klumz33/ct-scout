// src/ct_log/types.rs
use serde::{Deserialize, Serialize};

/// Response from CT log's get-sth endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedTreeHead {
    pub tree_size: u64,
    pub timestamp: u64,
    pub sha256_root_hash: String,
    #[serde(default)]
    pub tree_head_signature: String,
}

/// Single entry from CT log's get-entries endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub leaf_input: String,  // base64-encoded DER certificate
    pub extra_data: String,  // base64-encoded certificate chain
}

/// Response wrapper for get-entries endpoint
#[derive(Debug, Serialize, Deserialize)]
pub struct GetEntriesResponse {
    pub entries: Vec<LogEntry>,
}

/// Google's CT log list V3 format
#[derive(Debug, Serialize, Deserialize)]
pub struct LogListV3 {
    pub operators: Vec<Operator>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Operator {
    pub name: String,
    #[serde(default)]
    pub email: Vec<String>,
    #[serde(default)]
    pub logs: Vec<LogInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogInfo {
    #[serde(default)]
    pub description: String,
    pub log_id: Option<String>,
    pub key: Option<String>,
    #[serde(default)]
    pub url: String,
    pub mmd: Option<u64>,
    #[serde(default)]
    pub state: Option<StateWrapper>,
    pub temporal_interval: Option<TemporalInterval>,
}

/// State wrapper that can contain different state types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateWrapper {
    #[serde(default)]
    pub usable: Option<StateTimestamp>,
    #[serde(default)]
    pub readonly: Option<StateTimestamp>,
    #[serde(default)]
    pub retired: Option<StateTimestamp>,
    #[serde(default)]
    pub rejected: Option<StateTimestamp>,
    #[serde(default)]
    pub qualified: Option<StateTimestamp>,
    #[serde(default)]
    pub pending: Option<StateTimestamp>,
}

impl StateWrapper {
    /// Check if this state indicates the log is usable (actively accepting new entries)
    pub fn is_usable(&self) -> bool {
        self.usable.is_some() || self.qualified.is_some()
    }

    /// Check if this state indicates the log is readonly (frozen but may have recent entries)
    pub fn is_readonly(&self) -> bool {
        self.readonly.is_some()
    }

    /// Check if this state indicates the log is retired (shut down, historical only)
    pub fn is_retired(&self) -> bool {
        self.retired.is_some()
    }

    /// Check if this state indicates the log is rejected (not trusted)
    pub fn is_rejected(&self) -> bool {
        self.rejected.is_some()
    }

    /// Check if this state indicates the log is pending (not yet in service)
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Check if log is acceptable to monitor based on configuration
    pub fn is_acceptable(&self, include_readonly: bool, include_pending: bool) -> bool {
        self.is_usable()
            || (include_readonly && self.is_readonly())
            || (include_pending && self.is_pending())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTimestamp {
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalInterval {
    pub start_inclusive: Option<String>,
    pub end_exclusive: Option<String>,
}
