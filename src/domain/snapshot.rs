use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::{Display, EnumString};

/// Status of a snapshot operation (ETSI GS CIM 009 clause 5.2.41, status member).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum SnapshotStatus {
    Preparing,
    Success,
    Partial,
    Empty,
    Failure,
}

/// NGSI-LD Snapshot representation (ETSI GS CIM 009 clause 5.2.41).
///
/// A Snapshot captures the results of one or more entity queries and/or
/// temporal queries at a point in time. Used for offline access and
/// bulk data export.
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

/// Per-query execution result within a snapshot (ETSI GS CIM 009 clause 5.2.42).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResultDetails {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Input for POST /snapshots.
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

/// Input for PATCH /snapshots/{id}.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPatch {
    #[serde(default)]
    pub snapshot_lifetime: Option<String>,
    #[serde(default)]
    pub snapshot_priority: Option<i16>,
}
