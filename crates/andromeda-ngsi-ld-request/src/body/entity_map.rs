//! HTTP request model for updating an `EntityMap`.

use andromeda_ngsi_ld_lexicon::time::instant_serde;
use andromeda_ngsi_ld_model::request_body::RequestBody;
use chrono::{DateTime, Utc};
use serde::{Deserialize, de::IgnoredAny};

/// Request body for `PATCH /entityMaps/{id}` (clauses 5.14.2 and 6.32.3.2).
///
/// Clause 5.14.2.3 takes "A JSON-LD document", so an `application/ld+json`
/// body names its context in a top-level `@context`, which decoding skips.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateEntityMapBody {
    /// New expiration time, read as every instant a request carries (clause 4.6.3).
    #[serde(rename = "expiresAt", default, deserialize_with = "instant_serde::optional::deserialize")]
    pub expires_at: Option<DateTime<Utc>>,
    /// Output-only member accepted and ignored as clause 5.14.2.4 requires.
    #[serde(rename = "entityMap")]
    _entity_map: Option<IgnoredAny>,
    /// Output-only member accepted and ignored as clause 5.14.2.4 requires.
    #[serde(rename = "linkedMaps")]
    _linked_maps: Option<IgnoredAny>,
    /// Output-only member accepted and ignored as clause 5.14.2.4 requires.
    #[serde(rename = "type")]
    _type: Option<IgnoredAny>,
    /// Output-only member accepted and ignored as clause 5.14.2.4 requires.
    #[serde(rename = "id")]
    _id: Option<IgnoredAny>,
}

impl RequestBody for UpdateEntityMapBody {}

#[cfg(test)]
mod tests {
    use crate::body::entity_map::UpdateEntityMapBody;
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use andromeda_ngsi_ld_model::request_body::RequestBody;

    #[test]
    fn output_only_members_are_accepted_without_becoming_domain_values() {
        let body: UpdateEntityMapBody = serde_json::from_value(serde_json::json!({
            "expiresAt": "2026-01-01T00:00:00Z",
            "entityMap": {"anything": true},
            "linkedMaps": ["anything"],
            "type": "EntityMap",
            "id": "urn:ngsi-ld:EntityMap:one"
        }))
        .expect("output-only members are ignored");
        assert!(body.expires_at.is_some());
    }

    /// Clause 4.6.3 admits at most six fractional-second digits.
    #[test]
    fn an_expires_at_beyond_six_fractional_digits_is_refused() {
        assert!(UpdateEntityMapBody::from_request_text(br#"{"expiresAt": "2030-01-01T00:00:00.123456789Z"}"#).is_err());
        assert!(UpdateEntityMapBody::from_request_text(br#"{"expiresAt": "2030-01-01T00:00:00.1234567Z"}"#).is_err());
        assert!(UpdateEntityMapBody::from_request_text(br#"{"expiresAt": "2030-01-01T00:00:00.123456Z"}"#).is_ok());
    }

    #[test]
    fn an_absent_expires_at_reads_as_none() {
        pretty_assertions::assert_eq!(UpdateEntityMapBody::from_request_text(b"{}").expect("an empty update").expires_at, None);
    }

    #[test]
    fn an_unknown_member_is_rejected() {
        assert!(serde_json::from_value::<UpdateEntityMapBody>(serde_json::json!({"bogus": true})).is_err());
    }

    /// Clause 5.14.2.3: the body is a JSON-LD document, whose top-level
    /// `@context` is not a member of the update.
    #[test]
    fn a_top_level_context_is_skipped_when_the_body_is_read_from_its_text() {
        let body = UpdateEntityMapBody::from_request_text(
            br#"{"@context": ["https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"], "expiresAt": "2030-01-01T00:00:00Z"}"#,
        )
        .expect("the context is skipped");
        pretty_assertions::assert_eq!(
            body.expires_at,
            Some("2030-01-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("a valid timestamp").get())
        );
    }

    #[test]
    fn an_unknown_member_is_rejected_when_the_body_is_read_from_its_text() {
        assert!(UpdateEntityMapBody::from_request_text(br#"{"bogus": true}"#).is_err());
    }
}
