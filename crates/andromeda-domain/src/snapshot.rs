//! The Snapshot resource and its notification payload (ETSI GS CIM 009 clauses 5.2.41 to 5.3.4).

use crate::{
    attribute::name::AttributeName,
    constant_string,
    geo_rel::GeoRel,
    geometry::GeometryType,
    ngsi_ld_urn::{SnapshotId, SnapshotNotificationId, SnapshotNotificationIdParseError},
};
use chrono::{DateTime, Utc};
use compact_str::CompactString;
use derive_more::Display;
use iso8601_duration::Duration as Iso8601Duration;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;
use strum::{Display as StrumDisplay, EnumIter, EnumString, IntoStaticStr};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

constant_string!(SnapshotNotificationType, "SnapshotNotification");

/// Status of a snapshot operation (ETSI GS CIM 009 clause 5.2.41, `snapshotStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, StrumDisplay, EnumString, EnumIter, IntoStaticStr)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum SnapshotStatus {
    /// The snapshot's queries are still running.
    Preparing,
    /// Every query completed and produced results.
    Success,
    /// Some queries completed and some failed.
    Partial,
    /// Every query completed and none matched anything.
    Empty,
    /// Every query failed.
    Failure,
}

/// Status of an individual query or temporal-query execution (ETSI GS CIM 009 clause 5.2.42).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, StrumDisplay, EnumString)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum ExecutionResultStatus {
    /// The query completed and produced results.
    Success,
    /// The query failed; see the accompanying problem details.
    Failure,
    /// The query completed and matched nothing.
    Empty,
}

/// Snapshot priority (`1..=10`, default `5`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
#[serde(try_from = "u8", into = "u8")]
pub struct SnapshotPriority(u8);

impl SnapshotPriority {
    /// Lowest priority the spec allows.
    pub const MIN: u8 = 1;
    /// Highest priority the spec allows.
    pub const MAX: u8 = 10;
    /// Priority applied when the request does not state one.
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

// Hand-written because no derive can express it: `#[derive(Default)]` on a
// newtype forwards to the inner type, and `u8::default()` is `0`, which is
// outside the valid `MIN..=MAX` range and would make the default value one this
// type refuses to construct through `TryFrom`.
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
    /// The value sits outside the `1..=10` range clause 5.2.41 allows.
    #[error("snapshotPriority must be between 1 and 10, got {0}")]
    OutOfRange(i64),
}

/// NGSI-LD KeyValuePair (clause 5.2.22) used by `receiverInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyValuePair {
    /// The entry's key.
    pub key: CompactString,
    /// The entry's value.
    pub value: CompactString,
}

/// NGSI-LD Snapshot representation (ETSI GS CIM 009 clause 5.2.41).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Generated identifier of this snapshot.
    pub id: SnapshotId,
    /// Entity queries the snapshot materialises.
    pub snapshot_queries: Option<Vec<Value>>,
    /// Temporal queries the snapshot materialises.
    pub snapshot_temporal_queries: Option<Vec<Value>>,
    /// How long the snapshot stays usable after its last use.
    pub snapshot_lifetime: Option<Iso8601Duration>,
    /// Execution priority, `1..=10`.
    pub snapshot_priority: SnapshotPriority,
    /// Endpoint notified when the snapshot's status changes.
    pub endpoint: Option<Url>,
    /// Headers to send with the status notification.
    pub receiver_info: Option<Vec<KeyValuePair>>,
    /// Current status of the snapshot.
    pub status: SnapshotStatus,
    /// Per-entity-query execution details.
    pub queries_details: Option<Vec<ExecutionResultDetails>>,
    /// Per-temporal-query execution details.
    pub temporal_queries_details: Option<Vec<ExecutionResultDetails>>,
    /// When the snapshot was requested.
    pub created_at: DateTime<Utc>,
    /// When it was last changed.
    pub modified_at: DateTime<Utc>,
    /// When it stops being usable.
    pub expires_at: DateTime<Utc>,
    /// When it was last read, if it ever was.
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Per-query execution result (ETSI GS CIM 009 clause 5.2.42).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResultDetails {
    /// RFC 7807 problem details for a failed execution.
    pub problem_details: Option<Value>,
    /// Outcome of this single execution.
    pub result_status: ExecutionResultStatus,
}

/// Notification payload sent to a snapshot endpoint on status changes (clause 5.3.4).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotNotification {
    /// Unique notification identifier (`urn:ngsi-ld:SnapshotNotification:{uuid}`).
    pub id: SnapshotNotificationId,
    /// Always `SnapshotNotification`.
    #[serde(rename = "type")]
    pub type_: SnapshotNotificationType,
    /// Expiration time for the snapshot.
    pub expires_at: DateTime<Utc>,
    /// Timestamp corresponding to the instant when the notification is generated.
    pub notified_at: DateTime<Utc>,
    /// Identifier of the snapshot whose status change triggered the notification.
    pub snapshot_id: SnapshotId,
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
    /// The generated notification URN did not satisfy its own grammar.
    #[error("invalid generated snapshot notification id URN: {0}")]
    InvalidNotificationId(SnapshotNotificationIdParseError),
}

impl SnapshotStatus {
    /// Returns whether this status can transition to the given target status.
    ///
    /// Only `Preparing` can transition to a terminal state. Terminal states
    /// (`Success`, `Partial`, `Empty`, `Failure`) cannot transition further.
    pub const fn can_transition_to(self, target: SnapshotStatus) -> bool {
        matches!(
            (self, target),
            (SnapshotStatus::Preparing, SnapshotStatus::Success)
                | (SnapshotStatus::Preparing, SnapshotStatus::Partial)
                | (SnapshotStatus::Preparing, SnapshotStatus::Empty)
                | (SnapshotStatus::Preparing, SnapshotStatus::Failure)
        )
    }
}

/// Parsed snapshot query (spec 5.2.23, Query type).
///
/// Used to deserialize elements of `snapshotQueries` and `snapshotTemporalQueries`.
/// Non-temporal queries have `temporal_q: None`; temporal queries include it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotQuery {
    /// Entity selectors restricting the query by type, ID, or ID pattern.
    pub entities: Option<Vec<SnapshotEntitySelector>>,
    /// NGSI-LD query expression for property-level filtering.
    pub q: Option<String>,
    /// Geo-spatial query filter.
    pub geo_q: Option<SnapshotGeoQuery>,
    /// Scope query string (comma = OR, semicolon = AND).
    pub scope_q: Option<String>,
    /// Attribute names to include in the result.
    pub attrs: Option<Vec<AttributeName>>,
    /// Attribute names to exclude from the result.
    pub omit: Option<Vec<AttributeName>>,
    /// Temporal query parameters (present only in `snapshotTemporalQueries`).
    pub temporal_q: Option<SnapshotTemporalQuery>,
}

/// Entity selector within a snapshot query (spec 5.2.23).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotEntitySelector {
    /// Specific entity IDs to match.
    pub id: Option<Vec<String>>,
    /// Regex pattern for matching entity IDs.
    pub id_pattern: Option<String>,
    /// Entity type to match.
    #[serde(rename = "type")]
    pub type_: Option<String>,
}

/// Geo-spatial query within a snapshot query (spec 5.2.23).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotGeoQuery {
    /// Spatial relationship (clause 4.10).
    pub georel: GeoRel,
    /// GeoJSON geometry type (RFC 7946 section 1.4).
    pub geometry: GeometryType,
    /// GeoJSON coordinates.
    pub coordinates: Value,
    /// IRI of the GeoProperty to query against.
    pub geoproperty: Option<String>,
}

/// Temporal query within a snapshot temporal query (spec 5.2.23).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotTemporalQuery {
    /// Temporal relationship ("before", "after", "between").
    pub timerel: String,
    /// Reference time point (ISO 8601).
    pub time_at: String,
    /// End time point for "between" queries (ISO 8601).
    pub end_time_at: Option<String>,
    /// Maximum number of temporal instances per attribute.
    pub last_n: Option<u32>,
    /// Temporal property to filter on ("observedAt", "createdAt", etc.).
    pub timeproperty: Option<String>,
    /// Aggregation methods (comma-separated).
    pub aggr_methods: Option<String>,
    /// ISO 8601 duration for aggregation bucket size.
    pub aggr_period_duration: Option<String>,
}

impl Snapshot {
    /// Build a [`SnapshotNotification`] from the snapshot state.
    pub fn build_notification(&self) -> Result<SnapshotNotification, SnapshotNotificationBuildError> {
        let id = format!("urn:ngsi-ld:SnapshotNotification:{}", Uuid::new_v4())
            .parse::<SnapshotNotificationId>()
            .map_err(SnapshotNotificationBuildError::InvalidNotificationId)?;
        // clone: the notification owns its copy of the snapshot's id and
        // execution details, and the snapshot itself is only borrowed here.
        Ok(SnapshotNotification {
            id,
            type_: SnapshotNotificationType,
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
    /// Client-supplied identifier; the broker generates one when absent.
    #[serde(default)]
    pub id: Option<SnapshotId>,
    /// Entity queries to materialise.
    #[serde(default)]
    pub snapshot_queries: Option<Vec<Value>>,
    /// Temporal queries to materialise.
    #[serde(default)]
    pub snapshot_temporal_queries: Option<Vec<Value>>,
    /// How long the snapshot should stay usable.
    #[serde(default)]
    pub snapshot_lifetime: Option<Iso8601Duration>,
    /// Requested execution priority, validated into [`SnapshotPriority`].
    #[serde(default)]
    pub snapshot_priority: Option<i16>,
    /// Endpoint to notify on status changes.
    #[serde(default)]
    pub endpoint: Option<Url>,
    /// Headers to send with the status notification.
    #[serde(default)]
    pub receiver_info: Option<Vec<KeyValuePair>>,
}

/// Input for `PATCH /snapshots/{id}`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPatch {
    /// New lifetime, when the patch changes it.
    #[serde(default)]
    pub snapshot_lifetime: Option<Iso8601Duration>,
    /// New priority, when the patch changes it.
    #[serde(default)]
    pub snapshot_priority: Option<i16>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use crate::{
        geo_rel::GeoRel,
        geometry::GeometryType,
        ngsi_ld_urn::SnapshotId,
        snapshot::{SnapshotPriority, SnapshotStatus},
    };
    use iso8601_duration::Duration as Iso8601Duration;
    use strum::IntoEnumIterator;

    /// Labels of the PostgreSQL `snapshot_status` enum, verbatim from
    /// `migrations/tenant/0003_types.sql`.
    const SNAPSHOT_STATUS_LABELS: [(&str, SnapshotStatus); 5] = [
        ("preparing", SnapshotStatus::Preparing),
        ("success", SnapshotStatus::Success),
        ("partial", SnapshotStatus::Partial),
        ("empty", SnapshotStatus::Empty),
        ("failure", SnapshotStatus::Failure),
    ];

    #[test]
    fn snapshot_status_parses_from_its_postgres_label() {
        for (label, expected) in SNAPSHOT_STATUS_LABELS {
            pretty_assertions::assert_eq!(label.parse::<SnapshotStatus>().expect(label), expected);
        }
    }

    #[test]
    fn snapshot_status_renders_the_label_postgres_expects() {
        for (label, variant) in SNAPSHOT_STATUS_LABELS {
            pretty_assertions::assert_eq!(variant.to_string(), label);
            pretty_assertions::assert_eq!(<&str>::from(variant), label);
        }
    }

    #[test]
    fn snapshot_status_round_trips_through_its_label_for_every_variant() {
        // Exhaustive over `EnumIter`, replacing the type-level guarantee
        // `sqlx::Type` used to give.
        for variant in SnapshotStatus::iter() {
            pretty_assertions::assert_eq!(variant.to_string().parse::<SnapshotStatus>().expect("round trip"), variant);
            assert!(
                SNAPSHOT_STATUS_LABELS.iter().any(|(_, known)| *known == variant),
                "{variant:?} has no PostgreSQL label in snapshot_status"
            );
        }
    }

    #[test]
    fn an_unknown_snapshot_status_label_is_a_parse_error_not_a_panic() {
        assert!("not-a-status".parse::<SnapshotStatus>().is_err());
    }

    #[test]
    fn snapshot_urn_accepts_snapshot_prefix() {
        assert!("urn:ngsi-ld:Snapshot:abc".parse::<SnapshotId>().is_ok());
    }

    #[test]
    fn snapshot_urn_rejects_other_prefix() {
        assert!("urn:ngsi-ld:EntityMap:abc".parse::<SnapshotId>().is_err());
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
        pretty_assertions::assert_eq!(serialized, "\"preparing\"");
    }

    #[test]
    fn status_preparing_transitions_to_success() {
        assert!(SnapshotStatus::Preparing.can_transition_to(SnapshotStatus::Success));
    }

    #[test]
    fn status_preparing_transitions_to_all_terminal() {
        assert!(SnapshotStatus::Preparing.can_transition_to(SnapshotStatus::Success));
        assert!(SnapshotStatus::Preparing.can_transition_to(SnapshotStatus::Partial));
        assert!(SnapshotStatus::Preparing.can_transition_to(SnapshotStatus::Empty));
        assert!(SnapshotStatus::Preparing.can_transition_to(SnapshotStatus::Failure));
    }

    #[test]
    fn status_terminal_cannot_transition() {
        for terminal in [SnapshotStatus::Success, SnapshotStatus::Partial, SnapshotStatus::Empty, SnapshotStatus::Failure] {
            assert!(!terminal.can_transition_to(SnapshotStatus::Preparing));
            assert!(!terminal.can_transition_to(SnapshotStatus::Success));
            assert!(!terminal.can_transition_to(SnapshotStatus::Partial));
            assert!(!terminal.can_transition_to(SnapshotStatus::Empty));
            assert!(!terminal.can_transition_to(SnapshotStatus::Failure));
        }
    }

    #[test]
    fn parse_snapshot_query_minimal() {
        use crate::snapshot::SnapshotQuery;
        let json = r#"{"entities": [{"type": "Vehicle"}]}"#;
        let query: SnapshotQuery = serde_json::from_str(json).expect("parse");
        assert!(query.entities.is_some());
        let entities = query.entities.unwrap();
        pretty_assertions::assert_eq!(entities.len(), 1);
        pretty_assertions::assert_eq!(entities[0].type_.as_deref(), Some("Vehicle"));
        assert!(query.q.is_none());
        assert!(query.temporal_q.is_none());
    }

    #[test]
    fn parse_snapshot_query_full() {
        use crate::snapshot::SnapshotQuery;
        let json = r#"{
            "entities": [{"type": "Vehicle", "id": ["urn:ngsi-ld:Vehicle:1"], "idPattern": "urn:.*"}],
            "q": "speed>50",
            "geoQ": {"georel": "within", "geometry": "Polygon", "coordinates": [[[0,0],[1,0],[1,1],[0,1],[0,0]]], "geoproperty": "location"},
            "scopeQ": "/Madrid",
            "attrs": ["speed", "fuel"],
            "omit": ["name"]
        }"#;
        let query: SnapshotQuery = serde_json::from_str(json).expect("parse");
        assert!(query.entities.is_some());
        pretty_assertions::assert_eq!(query.q.as_deref(), Some("speed>50"));
        assert!(query.geo_q.is_some());
        let geo = query.geo_q.unwrap();
        pretty_assertions::assert_eq!(geo.georel, GeoRel::Within);
        pretty_assertions::assert_eq!(geo.geometry, GeometryType::Polygon);
        pretty_assertions::assert_eq!(query.scope_q.as_deref(), Some("/Madrid"));
        pretty_assertions::assert_eq!(query.attrs.unwrap().len(), 2);
        pretty_assertions::assert_eq!(query.omit.unwrap().len(), 1);
    }

    #[test]
    fn parse_temporal_query() {
        use crate::snapshot::SnapshotQuery;
        let json = r#"{
            "entities": [{"type": "Vehicle"}],
            "temporalQ": {
                "timerel": "after",
                "timeAt": "2024-01-01T00:00:00Z",
                "lastN": 10,
                "timeproperty": "observedAt"
            }
        }"#;
        let query: SnapshotQuery = serde_json::from_str(json).expect("parse");
        assert!(query.temporal_q.is_some());
        let tq = query.temporal_q.unwrap();
        pretty_assertions::assert_eq!(tq.timerel, "after");
        pretty_assertions::assert_eq!(tq.time_at, "2024-01-01T00:00:00Z");
        pretty_assertions::assert_eq!(tq.last_n, Some(10));
        pretty_assertions::assert_eq!(tq.timeproperty.as_deref(), Some("observedAt"));
    }

    #[test]
    fn parse_snapshot_query_rejects_invalid() {
        use crate::snapshot::SnapshotQuery;
        let result = serde_json::from_str::<SnapshotQuery>("not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn the_default_snapshot_priority_is_five_and_stays_valid() {
        let default = SnapshotPriority::default();
        pretty_assertions::assert_eq!(default, SnapshotPriority::DEFAULT);
        pretty_assertions::assert_eq!(default.as_u8(), 5);
        assert!((SnapshotPriority::MIN..=SnapshotPriority::MAX).contains(&default.as_u8()));
        // `u8::default()` is 0, which the range rejects, so a derived `Default`
        // would produce a value `TryFrom` refuses to build.
        assert!(SnapshotPriority::try_from(u8::default()).is_err());
    }
}
