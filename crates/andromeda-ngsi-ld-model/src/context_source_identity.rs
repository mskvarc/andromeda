//! The identity a Context Source answers `/info/sourceIdentity` with (clauses 5.2.40 and 6.33.3.1).

use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::{
    constant_string,
    context_source_pseudonym::ContextSourcePseudonym,
    duration::Iso8601Duration,
    identifier::Identifier,
    time::instant_serde,
};
use chrono::{DateTime, Utc};
use serde::Serialize;

constant_string!(pub ContextSourceIdentityType, "ContextSourceIdentity");

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
    pub id: Identifier,
    /// Always `"ContextSourceIdentity"`.
    #[serde(rename = "type")]
    pub type_: ContextSourceIdentityType,
    /// The pseudonym used to identify this source in a `Via` chain.
    pub context_source_alias: ContextSourcePseudonym,
    /// Total duration the source has been available, as an ISO 8601 duration.
    pub context_source_uptime: Iso8601Duration,
    /// The current time observed at the source.
    #[serde(with = "instant_serde::required")]
    pub context_source_time_at: DateTime<Utc>,
    /// Instance-specific configuration in raw JSON.
    ///
    /// Table 5.2.40-1 forbids interpreting this member as JSON-LD, so it is
    /// carried as an opaque value and never expanded or compacted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_source_extras: Option<JsonLiteral>,
}

#[cfg(test)]
mod tests {
    use crate::context_source_identity::{ContextSourceIdentity, ContextSourceIdentityType};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::duration::Iso8601Duration;
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use std::time::Duration;

    fn identity(extras: Option<serde_json::Value>) -> ContextSourceIdentity {
        let extras = extras.map(|extras| JsonLiteral::admit(&extras.to_string()).expect("a JSON literal"));
        ContextSourceIdentity {
            id: "urn:ngsi-ld:ContextSourceIdentity:andromeda".parse().expect("valid context source id"),
            type_: ContextSourceIdentityType,
            context_source_alias: "andromeda".parse().expect("valid context source alias"),
            context_source_uptime: Iso8601Duration::from_std(Duration::from_secs(90_061)),
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
}
