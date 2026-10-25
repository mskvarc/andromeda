//! The Snapshot resource and its notification payload (ETSI GS CIM 009 clauses 5.2.41 and 5.3.4).

use crate::{key_value_pair::KeyValuePair, query::body::Query};
use andromeda_ngsi_ld_lexicon::{constant_string, duration::Iso8601Duration, identifier::Identifier, ngsi_ld_urn::SnapshotNotificationId, time::instant_serde};
use andromeda_ngsi_ld_problem_details::problem_detail::ProblemDetail;
use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
use chrono::{DateTime, Utc};
use derive_more::{Display, Into};
use iri_rs::Iri;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use strum::{Display as StrumDisplay, EnumIter, EnumString, IntoStaticStr};
use thiserror::Error;
use typed_builder::TypedBuilder;
use url::Url;

constant_string!(SnapshotNotificationType, "SnapshotNotification");
constant_string!(SnapshotType, "Snapshot");

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, Into)]
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
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self.0
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

// `derive_more::TryFrom` reads an enum's discriminants; this is a newtype whose
// validity is a range rather than a variant set, so the bound check is written
// out. Clause 5.2.41 states the range as `1..=10`, which fits a `u8`; the
// persistence layer widens the value to the `SMALLINT` its column stores.
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

/// Parsing or validation errors for [`SnapshotPriority`].
#[derive(Debug, Error)]
pub enum SnapshotPriorityParseError {
    /// The value sits outside the `1..=10` range clause 5.2.41 allows.
    #[error("snapshotPriority must be between 1 and 10, got {0}")]
    OutOfRange(i64),
}

/// NGSI-LD Snapshot representation (ETSI GS CIM 009 clause 5.2.41).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Fixed resource discriminator.
    #[serde(rename = "type")]
    #[builder(default = Default::default())]
    pub type_: SnapshotType,
    /// Generated identifier of this snapshot.
    pub id: Identifier,
    /// Entity queries the snapshot materialises.
    #[builder(default = Default::default())]
    pub snapshot_queries: Option<Vec<Query>>,
    /// Temporal queries the snapshot materialises.
    #[builder(default = Default::default())]
    pub snapshot_temporal_queries: Option<Vec<Query>>,
    /// How long the snapshot stays usable after its last use.
    #[builder(default = Default::default())]
    pub snapshot_lifetime: Option<Iso8601Duration>,
    /// Execution priority, `1..=10`.
    pub snapshot_priority: SnapshotPriority,
    /// Endpoint notified when the snapshot's status changes.
    #[builder(default = Default::default())]
    pub endpoint: Option<Url>,
    /// Headers to send with the status notification.
    #[builder(default = Default::default())]
    pub receiver_info: Option<Vec<KeyValuePair>>,
    /// Current status of the snapshot, the `snapshotStatus` member of
    /// Table 5.2.41-2.
    #[serde(rename = "snapshotStatus")]
    pub status: SnapshotStatus,
    /// Per-entity-query execution details, the `snapshotQueriesDetails`
    /// member of Table 5.2.41-2.
    #[serde(rename = "snapshotQueriesDetails")]
    #[builder(default = Default::default())]
    pub queries_details: Option<Vec<ExecutionResultDetails>>,
    /// Per-temporal-query execution details, the
    /// `snapshotTemporalQueriesDetails` member of Table 5.2.41-2.
    #[serde(rename = "snapshotTemporalQueriesDetails")]
    #[builder(default = Default::default())]
    pub temporal_queries_details: Option<Vec<ExecutionResultDetails>>,
    /// When the snapshot was requested.
    #[serde(with = "instant_serde::required")]
    pub created_at: DateTime<Utc>,
    /// When it was last changed.
    #[serde(with = "instant_serde::required")]
    pub modified_at: DateTime<Utc>,
    /// When it stops being usable.
    #[serde(with = "instant_serde::required")]
    pub expires_at: DateTime<Utc>,
    /// When it was last read, if it ever was.
    #[builder(default = Default::default())]
    #[serde(with = "instant_serde::optional")]
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Per-query execution result (ETSI GS CIM 009 clause 5.2.42).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResultDetails {
    /// RFC 7807 problem details for a failed execution.
    pub problem_details: Option<ProblemDetail>,
    /// Outcome of this single execution.
    pub result_status: ExecutionResultStatus,
}

/// Notification payload sent to a snapshot endpoint on status changes (clause 5.3.4).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotNotification {
    /// The core context the notification is written under: it is sent as
    /// `application/ld+json`, whose body carries its `@context` (clause
    /// 6.3.6).
    #[serde(rename = "@context")]
    #[builder(default = NGSI_LD_CORE_CONTEXT_URL)]
    pub context: Iri<&'static str>,
    /// Unique notification identifier (`urn:ngsi-ld:SnapshotNotification:{uuid}`).
    pub id: SnapshotNotificationId,
    /// Always `SnapshotNotification`.
    #[serde(rename = "type")]
    #[builder(default = Default::default())]
    pub type_: SnapshotNotificationType,
    /// Expiration time for the snapshot.
    #[serde(with = "instant_serde::required")]
    pub expires_at: DateTime<Utc>,
    /// Timestamp corresponding to the instant when the notification is generated.
    #[serde(with = "instant_serde::required")]
    pub notified_at: DateTime<Utc>,
    /// Identifier of the snapshot whose status change triggered the notification.
    pub snapshot_id: Identifier,
    /// Priority of the snapshot (1..=10).
    pub snapshot_priority: SnapshotPriority,
    /// Per-query execution details, if available.
    #[builder(default = Default::default())]
    pub snapshot_queries_details: Option<Vec<ExecutionResultDetails>>,
    /// Current snapshot status (one of success, partial, failure, empty).
    pub snapshot_status: SnapshotStatus,
    /// Per-temporal-query execution details, if available.
    #[builder(default = Default::default())]
    pub temporal_snapshot_queries_details: Option<Vec<ExecutionResultDetails>>,
}

impl SnapshotStatus {
    /// Returns whether this status can transition to the given target status.
    ///
    /// Only `Preparing` can transition to a terminal state. Terminal states
    /// (`Success`, `Partial`, `Empty`, `Failure`) cannot transition further.
    #[must_use]
    pub const fn can_transition_to(self, target: SnapshotStatus) -> bool {
        matches!(
            (self, target),
            (
                SnapshotStatus::Preparing,
                SnapshotStatus::Success | SnapshotStatus::Partial | SnapshotStatus::Empty | SnapshotStatus::Failure
            )
        )
    }
}

impl Snapshot {
    /// Build a [`SnapshotNotification`] from the snapshot state.
    #[must_use]
    pub fn build_notification(&self) -> SnapshotNotification {
        let id = SnapshotNotificationId::mint();
        // clone: the notification owns its copy of the snapshot's id and
        // execution details, and the snapshot itself is only borrowed here.
        SnapshotNotification::builder()
            .id(id)
            .expires_at(self.expires_at)
            .notified_at(Utc::now())
            .snapshot_id(self.id.clone())
            .snapshot_priority(self.snapshot_priority)
            .snapshot_queries_details(self.queries_details.clone())
            .snapshot_status(self.status)
            .temporal_snapshot_queries_details(self.temporal_queries_details.clone())
            .build()
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        query::{body::Query, temporal_query::TemporalQuery},
        snapshot::{Snapshot, SnapshotNotification, SnapshotPriority, SnapshotStatus},
    };
    use andromeda_ngsi_ld_geometry::geo_rel::GeoRel;
    use andromeda_ngsi_ld_lexicon::{
        aggr_method::AggrMethod,
        duration::Iso8601Duration,
        identifier::Identifier,
        ngsi_ld_urn::SnapshotNotificationId,
        time::date_time::NgsiLdDateTime,
        time_prop::TimeProp,
        time_rel::TimeRel,
    };
    use andromeda_ngsi_ld_query::geo::reference_geometry::ReferenceGeometry;
    use chrono::{DateTime, TimeDelta, TimeZone, Utc};
    use std::num::NonZeroU32;

    #[test]
    fn an_unknown_snapshot_status_label_is_a_parse_error_not_a_panic() {
        assert!("not-a-status".parse::<SnapshotStatus>().is_err());
    }

    #[test]
    fn snapshot_urn_accepts_snapshot_prefix() {
        assert!("urn:ngsi-ld:Snapshot:abc".parse::<Identifier>().is_ok());
    }

    #[test]
    fn snapshot_urn_rejects_other_prefix() {
        assert!("https://example.com/snapshots/abc".parse::<Identifier>().is_ok());
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
        let json = r#"{"type":"Query","entities": [{"type": "Vehicle"}]}"#;
        let query: Query = serde_json::from_str(json).expect("parse");
        assert!(query.entities.is_some());
        let entities = query.entities.unwrap();
        pretty_assertions::assert_eq!(entities.len(), 1);
        pretty_assertions::assert_eq!(entities[0].type_.to_string(), "Vehicle");
        assert!(query.q.is_none());
        assert!(query.temporal_q.is_none());
    }

    #[test]
    fn parse_snapshot_query_full() {
        let json = r#"{
            "type": "Query",
            "entities": [{"type": "Vehicle", "id": ["urn:ngsi-ld:Vehicle:1"], "idPattern": "urn:.*"}],
            "q": "speed>50",
            "geoQ": {"georel": "within", "geometry": "Polygon", "coordinates": [[[0,0],[1,0],[1,1],[0,1],[0,0]]], "geoproperty": "location"},
            "scopeQ": "/Madrid",
            "attrs": ["speed", "fuel"],
            "omit": ["name"]
        }"#;
        let query: Query = serde_json::from_str(json).expect("parse");
        assert!(query.entities.is_some());
        pretty_assertions::assert_eq!(query.q.as_ref().map(ToString::to_string).as_deref(), Some("speed>50"));
        assert!(query.geo_q.is_some());
        let geo = query.geo_q.unwrap();
        pretty_assertions::assert_eq!(geo.georel, GeoRel::Within);
        let ReferenceGeometry::Read(geometry) = geo.reference else {
            panic!("JSON coordinates are read at decoding");
        };
        pretty_assertions::assert_eq!(serde_json::to_value(geometry.to_geojson()).expect("geometry serializes")["type"], "Polygon");
        pretty_assertions::assert_eq!(query.scope_q.as_ref().map(ToString::to_string).as_deref(), Some("/Madrid"));
        pretty_assertions::assert_eq!(query.attrs.unwrap().len(), 2);
        pretty_assertions::assert_eq!(query.omit.unwrap().len(), 1);
    }

    /// Build a `temporalQ` payload with the given members spliced in.
    fn temporal_q(members: &str) -> Result<TemporalQuery, serde_json::Error> {
        serde_json::from_str::<Query>(&format!(
            r#"{{"type":"Query","entities": [{{"type": "Vehicle"}}], "temporalQ": {{{members}}}}}"#
        ))
        .map(|query| query.temporal_q.expect("temporalQ present"))
    }

    /// The instant `2024-01-01T00:00:00Z`, as a `DateTime<Utc>`.
    fn january_first() -> DateTime<Utc> {
        "2024-01-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("valid instant").get()
    }

    #[test]
    fn a_snapshot_builder_defaults_every_optional_field() {
        let instant = "2024-01-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("valid instant").get();
        let snapshot = Snapshot::builder()
            .id("urn:ngsi-ld:Snapshot:1".parse::<Identifier>().expect("valid snapshot id"))
            .snapshot_priority(SnapshotPriority::DEFAULT)
            .status(SnapshotStatus::Preparing)
            .created_at(instant)
            .modified_at(instant)
            .expires_at(instant)
            .build();
        assert!(snapshot.snapshot_queries.is_none() && snapshot.snapshot_temporal_queries.is_none());
        assert!(snapshot.snapshot_lifetime.is_none() && snapshot.endpoint.is_none() && snapshot.receiver_info.is_none());
        assert!(snapshot.queries_details.is_none() && snapshot.temporal_queries_details.is_none());
        assert!(snapshot.last_used_at.is_none());
    }

    /// Table 5.2.41-2 names the output-only members `snapshotStatus`,
    /// `snapshotQueriesDetails` and `snapshotTemporalQueriesDetails`.
    #[test]
    fn the_output_only_members_carry_their_table_5_2_41_2_names() {
        let instant = "2024-01-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("valid instant").get();
        let snapshot = Snapshot::builder()
            .id("urn:ngsi-ld:Snapshot:1".parse::<Identifier>().expect("valid snapshot id"))
            .snapshot_priority(SnapshotPriority::DEFAULT)
            .status(SnapshotStatus::Preparing)
            .queries_details(Some(Vec::new()))
            .temporal_queries_details(Some(Vec::new()))
            .created_at(instant)
            .modified_at(instant)
            .expires_at(instant)
            .build();
        let value = serde_json::to_value(&snapshot).expect("a snapshot serialises");
        pretty_assertions::assert_eq!(value["snapshotStatus"], serde_json::json!("preparing"));
        assert!(value.get("status").is_none(), "{value}");
        assert!(value.get("snapshotQueriesDetails").is_some(), "{value}");
        assert!(value.get("snapshotTemporalQueriesDetails").is_some(), "{value}");
        assert!(
            value.get("queriesDetails").is_none() && value.get("temporalQueriesDetails").is_none(),
            "{value}"
        );
    }

    /// Clause 5.2.41 and 5.2.42: a Snapshot with a failed execution result
    /// decodes to the typed model, `problemDetails` included, and writes back
    /// the body it was parsed from.
    #[test]
    fn a_snapshot_round_trips_through_its_typed_model() {
        let body = serde_json::json!({
            "id": "urn:ngsi-ld:Snapshot:1",
            "type": "Snapshot",
            "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}],
            "snapshotLifetime": "PT1H",
            "snapshotPriority": 7,
            "endpoint": "http://localhost:9000/status",
            "receiverInfo": [{"key": "Authorization", "value": "Bearer token"}],
            "snapshotStatus": "partial",
            "snapshotQueriesDetails": [
                {"resultStatus": "success"},
                {
                    "resultStatus": "failure",
                    "problemDetails": {
                        "type": "https://uri.etsi.org/ngsi-ld/errors/InternalError",
                        "title": "Internal error",
                        "status": 500,
                        "detail": "the source is unavailable"
                    }
                }
            ],
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:05Z",
            "expiresAt": "2024-01-01T01:00:00Z",
            "lastUsedAt": "2024-01-01T00:00:06Z"
        });
        let snapshot = serde_json::from_value::<Snapshot>(body.clone()).expect("the body decodes to the typed Snapshot");
        pretty_assertions::assert_eq!(serde_json::to_value(&snapshot).expect("the snapshot serialises"), body);
    }

    /// The broker-minted snapshot timestamps keep their sub-second remainder
    /// rather than truncating it to whole seconds (ETSI GS CIM 009 v1.9.1 clause
    /// 4.6.3); a millisecond instant renders three fractional digits.
    #[test]
    fn snapshot_timestamps_serialize_with_sub_second_precision() {
        let instant = "2024-01-01T00:00:00.750Z".parse::<NgsiLdDateTime>().expect("valid instant").get();
        let snapshot = Snapshot::builder()
            .id("urn:ngsi-ld:Snapshot:1".parse::<Identifier>().expect("valid snapshot id"))
            .snapshot_priority(SnapshotPriority::DEFAULT)
            .status(SnapshotStatus::Preparing)
            .created_at(instant)
            .modified_at(instant)
            .expires_at(instant)
            .build();

        let rendered = serde_json::to_value(snapshot).expect("snapshot serializes");
        pretty_assertions::assert_eq!(rendered["createdAt"], "2024-01-01T00:00:00.750Z");
        pretty_assertions::assert_eq!(rendered["modifiedAt"], "2024-01-01T00:00:00.750Z");
        pretty_assertions::assert_eq!(rendered["expiresAt"], "2024-01-01T00:00:00.750Z");
    }

    /// Clause 4.6.3 caps a `DateTime` at six fractional digits, so a nanosecond
    /// instant is written at the microsecond.
    #[test]
    fn a_snapshot_notification_writes_its_notified_at_at_the_microsecond() {
        let instant = Utc.with_ymd_and_hms(2026, 10, 19, 16, 27, 34).single().expect("valid instant") + TimeDelta::nanoseconds(800_524_180);
        let notification = SnapshotNotification::builder()
            .id(SnapshotNotificationId::mint())
            .expires_at(instant)
            .notified_at(instant)
            .snapshot_id("urn:ngsi-ld:Snapshot:1".parse::<Identifier>().expect("valid snapshot id"))
            .snapshot_priority(SnapshotPriority::DEFAULT)
            .snapshot_status(SnapshotStatus::Success)
            .build();
        let rendered = serde_json::to_value(notification).expect("notification serializes");
        pretty_assertions::assert_eq!(rendered["notifiedAt"], "2026-10-19T16:27:34.800524Z");
        pretty_assertions::assert_eq!(rendered["expiresAt"], "2026-10-19T16:27:34.800524Z");
    }

    /// The notification stamped from the broker clock carries a `notifiedAt`
    /// the clause 4.6.3 reading of a received instant accepts.
    #[test]
    fn a_built_snapshot_notification_stamps_an_admissible_notified_at() {
        let snapshot = Snapshot::builder()
            .id("urn:ngsi-ld:Snapshot:1".parse::<Identifier>().expect("valid snapshot id"))
            .snapshot_priority(SnapshotPriority::DEFAULT)
            .status(SnapshotStatus::Success)
            .created_at(january_first())
            .modified_at(january_first())
            .expires_at(january_first())
            .build();
        let rendered = serde_json::to_value(snapshot.build_notification()).expect("notification serializes");
        let notified_at = rendered["notifiedAt"].as_str().expect("notifiedAt is a string");
        assert!(notified_at.parse::<NgsiLdDateTime>().is_ok(), "{notified_at}");
    }

    #[test]
    fn a_snapshot_temporal_query_parses_its_temporal_relation() {
        let tq = temporal_q(r#""timerel": "between", "timeAt": "2024-01-01T00:00:00Z", "endTimeAt": "2024-02-01T00:00:00Z""#).expect("parses");
        pretty_assertions::assert_eq!(tq.timerel, TimeRel::Between);
        pretty_assertions::assert_eq!(tq.time_at, january_first());
        pretty_assertions::assert_eq!(
            tq.end_time_at,
            Some("2024-02-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("valid instant").get())
        );
    }

    #[test]
    fn a_snapshot_temporal_query_rejects_an_unknown_temporal_relation() {
        assert!(temporal_q(r#""timerel": "during", "timeAt": "2024-01-01T00:00:00Z""#).is_err());
    }

    /// Clause 4.6.3: the trailing component "shall always be equal to the
    /// character 'Z'", so an offset is refused even where it names UTC.
    #[test]
    fn a_snapshot_temporal_query_refuses_an_offset_bearing_instant() {
        assert!(temporal_q(r#""timerel": "after", "timeAt": "2024-01-01T01:00:00+01:00""#).is_err());
        assert!(temporal_q(r#""timerel": "after", "timeAt": "2024-01-01T00:00:00+00:00""#).is_err());
    }

    #[test]
    fn a_snapshot_temporal_query_parses_its_aggregation_methods() {
        let tq = temporal_q(r#""timerel": "after", "timeAt": "2024-01-01T00:00:00Z", "aggrMethods": "avg,max""#).expect("parses");
        pretty_assertions::assert_eq!(tq.aggr_methods, Some(vec![AggrMethod::Avg, AggrMethod::Max]));
    }

    #[test]
    fn a_snapshot_temporal_query_rejects_an_unknown_aggregation_method() {
        assert!(temporal_q(r#""timerel": "after", "timeAt": "2024-01-01T00:00:00Z", "aggrMethods": "median""#).is_err());
    }

    #[test]
    fn a_snapshot_temporal_query_parses_its_time_property() {
        let tq = temporal_q(r#""timerel": "after", "timeAt": "2024-01-01T00:00:00Z", "timeproperty": "createdAt", "lastN": 10"#).expect("parses");
        pretty_assertions::assert_eq!(tq.timeproperty, Some(TimeProp::Created));
        pretty_assertions::assert_eq!(tq.last_n.map(NonZeroU32::get), Some(10));
    }

    #[test]
    fn parse_snapshot_query_rejects_invalid() {
        let result = serde_json::from_str::<Query>("not valid json");
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
