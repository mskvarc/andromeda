use chrono::{DateTime, Utc};
use compact_str::CompactString;
use derive_more::Display;
use iso8601_duration::Duration as Iso8601Duration;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;
use strum::{Display as StrumDisplay, EnumString};
use thiserror::Error;
use url::Url;
use urn::Urn;
use uuid::Uuid;

/// Status of a snapshot operation (ETSI GS CIM 009 clause 5.2.41, `snapshotStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, StrumDisplay, EnumString)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum SnapshotStatus {
    Preparing,
    Success,
    Partial,
    Empty,
    Failure,
}

/// Status of an individual query or temporal-query execution (ETSI GS CIM 009 clause 5.2.42).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, StrumDisplay, EnumString)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum ExecutionResultStatus {
    Success,
    Failure,
    Empty,
}

/// Snapshot priority (`1..=10`, default `5`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
#[serde(try_from = "u8", into = "u8")]
pub struct SnapshotPriority(u8);

impl SnapshotPriority {
    pub const MIN: u8 = 1;
    pub const MAX: u8 = 10;
    pub const DEFAULT: SnapshotPriority = SnapshotPriority(5);

    /// Returns the integer representation.
    pub const fn as_u8(self) -> u8 {
        self.0
    }

    /// Returns the integer representation as i16.
    pub const fn as_i16(self) -> i16 {
        self.0 as i16
    }
}

impl Default for SnapshotPriority {
    fn default() -> SnapshotPriority {
        SnapshotPriority::DEFAULT
    }
}

impl TryFrom<u8> for SnapshotPriority {
    type Error = SnapshotPriorityParseError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if (Self::MIN..=Self::MAX).contains(&value) {
            Ok(SnapshotPriority(value))
        } else {
            Err(SnapshotPriorityParseError::OutOfRange(value.into()))
        }
    }
}

impl TryFrom<i16> for SnapshotPriority {
    type Error = SnapshotPriorityParseError;

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        if (i16::from(Self::MIN)..=i16::from(Self::MAX)).contains(&value) {
            Ok(SnapshotPriority(value as u8))
        } else {
            Err(SnapshotPriorityParseError::OutOfRange(value.into()))
        }
    }
}

impl From<SnapshotPriority> for u8 {
    fn from(value: SnapshotPriority) -> u8 {
        value.0
    }
}

/// Parsing or validation errors for [`SnapshotPriority`].
#[derive(Debug, Error)]
pub enum SnapshotPriorityParseError {
    #[error("snapshotPriority must be between 1 and 10, got {0}")]
    OutOfRange(i64),
}

/// NGSI-LD KeyValuePair (clause 5.2.22) used by `receiverInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyValuePair {
    pub key: CompactString,
    pub value: CompactString,
}

/// NGSI-LD Snapshot representation (ETSI GS CIM 009 clause 5.2.41).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: Urn,
    pub snapshot_queries: Option<Vec<Value>>,
    pub snapshot_temporal_queries: Option<Vec<Value>>,
    pub snapshot_lifetime: Option<Iso8601Duration>,
    pub snapshot_priority: SnapshotPriority,
    pub endpoint: Option<Url>,
    pub receiver_info: Option<Vec<KeyValuePair>>,
    pub status: SnapshotStatus,
    pub queries_details: Option<Vec<ExecutionResultDetails>>,
    pub temporal_queries_details: Option<Vec<ExecutionResultDetails>>,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Per-query execution result (ETSI GS CIM 009 clause 5.2.42).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResultDetails {
    pub problem_details: Option<Value>,
    pub result_status: ExecutionResultStatus,
}

/// JSON-LD `@type` for snapshot notifications (Table 5.3.4-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapshotNotificationType {
    #[serde(rename = "SnapshotNotification")]
    SnapshotNotification,
}

/// Notification payload sent to a snapshot endpoint on status changes (clause 5.3.4).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotNotification {
    /// Unique notification identifier (`urn:ngsi-ld:SnapshotNotification:{uuid}`).
    pub id: Urn,
    /// Always `SnapshotNotification`.
    #[serde(rename = "type")]
    pub type_: SnapshotNotificationType,
    /// Expiration time for the snapshot.
    pub expires_at: DateTime<Utc>,
    /// Timestamp corresponding to the instant when the notification is generated.
    pub notified_at: DateTime<Utc>,
    /// Identifier of the snapshot whose status change triggered the notification.
    pub snapshot_id: Urn,
    /// Priority of the snapshot (1..=10).
    pub snapshot_priority: SnapshotPriority,
    /// Per-query execution details, if available.
    pub snapshot_queries_details: Option<Vec<ExecutionResultDetails>>,
    /// Current snapshot status (one of success, partial, failure, empty).
    pub snapshot_status: SnapshotStatus,
    /// Per-temporal-query execution details, if available.
    pub temporal_snapshot_queries_details: Option<Vec<ExecutionResultDetails>>,
}

/// Errors produced while building snapshot notifications.
#[derive(Debug, Error)]
pub enum SnapshotNotificationBuildError {
    #[error("invalid generated snapshot notification id URN: {0}")]
    InvalidNotificationId(urn::Error),
}

impl Snapshot {
    /// Build a [`SnapshotNotification`] from the snapshot state.
    pub fn build_notification(&self) -> Result<SnapshotNotification, SnapshotNotificationBuildError> {
        let id = format!("urn:ngsi-ld:SnapshotNotification:{}", Uuid::new_v4())
            .parse::<Urn>()
            .map_err(SnapshotNotificationBuildError::InvalidNotificationId)?;
        Ok(SnapshotNotification {
            id,
            type_: SnapshotNotificationType::SnapshotNotification,
            expires_at: self.expires_at,
            notified_at: Utc::now(),
            snapshot_id: self.id.clone(),
            snapshot_priority: self.snapshot_priority,
            snapshot_queries_details: self.queries_details.clone(),
            snapshot_status: self.status,
            temporal_snapshot_queries_details: self.temporal_queries_details.clone(),
        })
    }
}

/// Input for `POST /snapshots`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInput {
    #[serde(default)]
    pub id: Option<Urn>,
    #[serde(default)]
    pub snapshot_queries: Option<Vec<Value>>,
    #[serde(default)]
    pub snapshot_temporal_queries: Option<Vec<Value>>,
    #[serde(default)]
    pub snapshot_lifetime: Option<Iso8601Duration>,
    #[serde(default)]
    pub snapshot_priority: Option<i16>,
    #[serde(default)]
    pub endpoint: Option<Url>,
    #[serde(default)]
    pub receiver_info: Option<Vec<KeyValuePair>>,
}

/// Input for `PATCH /snapshots/{id}`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPatch {
    #[serde(default)]
    pub snapshot_lifetime: Option<Iso8601Duration>,
    #[serde(default)]
    pub snapshot_priority: Option<i16>,
}

/// Validate that a URN is a snapshot URN (`urn:ngsi-ld:Snapshot:<id>`).
pub fn validate_snapshot_urn(id: &Urn) -> Result<(), SnapshotUrnError> {
    if id.as_str().starts_with("urn:ngsi-ld:Snapshot:") {
        Ok(())
    } else {
        Err(SnapshotUrnError::InvalidKind)
    }
}

/// Errors for snapshot URN validation.
#[derive(Debug, Error)]
pub enum SnapshotUrnError {
    #[error("snapshot id must use the 'urn:ngsi-ld:Snapshot:' prefix")]
    InvalidKind,
}

#[cfg(test)]
mod tests {
    use crate::domain::snapshot::{SnapshotPriority, SnapshotStatus, validate_snapshot_urn};
    use iso8601_duration::Duration as Iso8601Duration;
    use urn::Urn;

    #[test]
    fn snapshot_urn_accepts_snapshot_prefix() {
        let id: Urn = "urn:ngsi-ld:Snapshot:abc".parse().expect("valid snapshot urn");
        assert!(validate_snapshot_urn(&id).is_ok());
    }

    #[test]
    fn snapshot_urn_rejects_other_prefix() {
        let id: Urn = "urn:ngsi-ld:EntityMap:abc".parse().expect("valid urn");
        assert!(validate_snapshot_urn(&id).is_err());
    }

    #[test]
    fn snapshot_lifetime_uses_crate_duration() {
        assert!("PT1H".parse::<Iso8601Duration>().is_ok());
        assert!("1H".parse::<Iso8601Duration>().is_err());
    }

    #[test]
    fn snapshot_priority_range_is_enforced() {
        assert!(SnapshotPriority::try_from(1u8).is_ok());
        assert!(SnapshotPriority::try_from(10u8).is_ok());
        assert!(SnapshotPriority::try_from(0u8).is_err());
        assert!(SnapshotPriority::try_from(11u8).is_err());
    }

    #[test]
    fn snapshot_status_serializes_preparing() {
        let serialized = serde_json::to_string(&SnapshotStatus::Preparing).expect("serialize");
        assert_eq!(serialized, "\"preparing\"");
    }
}
