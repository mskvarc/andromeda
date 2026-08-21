//! The identity a Context Source answers `/info/sourceIdentity` with (clauses 5.2.40 and 6.33.3.1).

use andromeda_ngsi_ld_lexicon::{constant_string, context_source_pseudonym::ContextSourcePseudonym, rfc3339, uri::Uri};
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

constant_string!(ContextSourceIdentityType, "ContextSourceIdentity");

/// The response body of the Context Source identity endpoint (Table 5.2.40-1).
///
/// The table separates the source's URI identifier from the pseudonym it
/// places in `Via` headers for loop detection, and makes the uptime and the
/// observation instant mandatory: an identity is a statement about the source
/// *now*, so a document is built per request rather than held once.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSourceIdentity {
    /// The identifier of this context source.
    pub id: Uri,
    /// Always `"ContextSourceIdentity"`.
    #[serde(rename = "type")]
    pub type_: ContextSourceIdentityType,
    /// The pseudonym used to identify this source in a `Via` chain.
    pub context_source_alias: ContextSourcePseudonym,
    /// Total duration the source has been available, as an ISO 8601 duration.
    pub context_source_uptime: Iso8601Duration,
    /// The current time observed at the source.
    #[serde(with = "rfc3339::datetime")]
    pub context_source_time_at: DateTime<Utc>,
    /// Instance-specific configuration in raw JSON.
    ///
    /// Table 5.2.40-1 forbids interpreting this member as JSON-LD, so it is
    /// carried as an opaque value and never expanded or compacted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_source_extras: Option<Value>,
}

/// Seconds in one day, for decomposing an uptime.
const SECONDS_PER_DAY: u64 = 86_400;
/// Seconds in one hour, for decomposing an uptime.
const SECONDS_PER_HOUR: u64 = 3_600;
/// Seconds in one minute, for decomposing an uptime.
const SECONDS_PER_MINUTE: u64 = 60;

/// Render an elapsed duration as the ISO 8601 duration `contextSourceUptime` carries.
///
/// Decomposed into days, hours, minutes and seconds so each component stays a
/// small whole number; only the seconds carry a millisecond fraction, so the
/// rendering is exact for any realistic uptime.
pub fn uptime_duration(elapsed: Duration) -> Iso8601Duration {
    let whole_seconds = elapsed.as_secs();
    let days = whole_seconds / SECONDS_PER_DAY;
    let hours = (whole_seconds % SECONDS_PER_DAY) / SECONDS_PER_HOUR;
    let minutes = (whole_seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE;
    let seconds = whole_seconds % SECONDS_PER_MINUTE;
    let millis = elapsed.subsec_millis();
    // f32 holds every whole number up to 2^24 exactly, far beyond any component here.
    let seconds_with_fraction = seconds as f32 + f32::from(u16::try_from(millis).unwrap_or(u16::MAX)) / 1_000.0;
    Iso8601Duration::new(0.0, 0.0, days as f32, hours as f32, minutes as f32, seconds_with_fraction)
}

#[cfg(test)]
mod tests {
    use crate::context_source_identity::{ContextSourceIdentity, ContextSourceIdentityType, uptime_duration};
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use std::time::Duration;

    fn identity(extras: Option<serde_json::Value>) -> ContextSourceIdentity {
        ContextSourceIdentity {
            id: "urn:ngsi-ld:ContextSourceIdentity:andromeda".parse().expect("valid context source id"),
            type_: ContextSourceIdentityType,
            context_source_alias: "andromeda".parse().expect("valid context source alias"),
            context_source_uptime: uptime_duration(Duration::from_secs(90_061)),
            context_source_time_at: Utc.with_ymd_and_hms(2026, 8, 21, 12, 0, 0).single().expect("valid instant"),
            context_source_extras: extras,
        }
    }

    #[test]
    fn the_identity_renders_every_mandatory_member_of_table_5_2_40_1() {
        let rendered = serde_json::to_value(identity(None)).expect("serialises");
        pretty_assertions::assert_eq!(rendered["id"], "urn:ngsi-ld:ContextSourceIdentity:andromeda");
        pretty_assertions::assert_eq!(rendered["type"], "ContextSourceIdentity");
        pretty_assertions::assert_eq!(rendered["contextSourceAlias"], "andromeda");
        pretty_assertions::assert_eq!(rendered["contextSourceUptime"], "P1DT1H1M1S");
        pretty_assertions::assert_eq!(rendered["contextSourceTimeAt"], "2026-08-21T12:00:00Z");
        assert!(rendered.get("contextSourceExtras").is_none());
        assert!(rendered.get("type_").is_none());
    }

    #[test]
    fn extras_are_carried_verbatim() {
        let extras = json!({"region": "eu-west", "nested": {"raw": [1, 2, 3]}});
        let rendered = serde_json::to_value(identity(Some(extras.clone()))).expect("serialises"); // clone: compared below
        pretty_assertions::assert_eq!(rendered["contextSourceExtras"], extras);
    }

    #[test]
    fn an_uptime_decomposes_into_iso_8601_components() {
        pretty_assertions::assert_eq!(uptime_duration(Duration::from_millis(1_500)).to_string(), "PT1.5S");
        pretty_assertions::assert_eq!(uptime_duration(Duration::from_secs(3_600)).to_string(), "PT1H");
        pretty_assertions::assert_eq!(uptime_duration(Duration::from_secs(2 * 86_400 + 30)).to_string(), "P2DT30S");
    }
}
