use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotStatus {
    Preparing,
    Success,
    Partial,
    Empty,
    Failure,
}

impl SnapshotStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::Success => "success",
            Self::Partial => "partial",
            Self::Empty => "empty",
            Self::Failure => "failure",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "preparing" => Some(Self::Preparing),
            "success" => Some(Self::Success),
            "partial" => Some(Self::Partial),
            "empty" => Some(Self::Empty),
            "failure" => Some(Self::Failure),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_queries: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_temporal_queries: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_lifetime: Option<String>,
    pub snapshot_priority: i16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver_info: Option<Vec<Value>>,
    pub status: SnapshotStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queries_details: Option<Vec<ExecutionResultDetails>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temporal_queries_details: Option<Vec<ExecutionResultDetails>>,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResultDetails {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Input for POST /snapshots
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInput {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub snapshot_queries: Option<Vec<Value>>,
    #[serde(default)]
    pub snapshot_temporal_queries: Option<Vec<Value>>,
    #[serde(default)]
    pub snapshot_lifetime: Option<String>,
    #[serde(default)]
    pub snapshot_priority: Option<i16>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub receiver_info: Option<Vec<Value>>,
}

/// Input for PATCH /snapshots/{id}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPatch {
    #[serde(default)]
    pub snapshot_lifetime: Option<String>,
    #[serde(default)]
    pub snapshot_priority: Option<i16>,
}
