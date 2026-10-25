//! The `@context` resources the broker caches, hosts, and serves (ETSI GS CIM 009 clause 5.13).

use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::instant_serde};
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoStaticStr};
use typed_builder::TypedBuilder;
use url::Url;

/// Kind of JSON-LD @context managed by the broker (ETSI GS CIM 009 clause 5.13).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Display, EnumString, EnumIter, IntoStaticStr)]
#[strum(serialize_all = "PascalCase")]
pub enum ContextKind {
    /// Fetched from its origin and held in the broker's cache.
    Cached,
    /// Uploaded to the broker, which serves it under its own URL.
    Hosted,
    /// Created by the broker when a payload inlined a context body.
    ImplicitlyCreated,
}

/// Detailed @context metadata response (for details=true).
#[derive(Debug, Serialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct ContextMetadata {
    /// Origin URL, absent for contexts the broker itself created.
    #[serde(rename = "URL")]
    #[builder(default = Default::default())]
    pub url: Option<Url>,
    /// The serving URL exactly as stored in the `url` column, kept verbatim.
    ///
    /// Clause 5.13.3.5 keys `numberOfHits` on the stored URL, and the in-process
    /// hit counter is bumped and flushed under that raw string. This field
    /// carries it unparsed so a `numberOfHits` fold keys on the same bytes: the
    /// re-serialized [`ContextMetadata::url`] cannot be used, because
    /// `url::Url`'s normalization (trailing slash, default-port removal,
    /// percent-encoding) would silently miss the counter for any serving URL
    /// that does not round-trip. Internal only, never serialized.
    #[serde(skip)]
    pub serving_iri: Option<IriBuf>,
    /// Identifier under which the broker serves this context.
    pub local_id: Identifier,
    /// How the broker came to hold this context.
    pub kind: ContextKind,
    /// When the broker first stored it.
    #[serde(with = "instant_serde::required")]
    pub created_at: DateTime<Utc>,
    /// When the cached copy stops being valid, if it expires at all.
    #[builder(default = Default::default())]
    #[serde(with = "instant_serde::optional")]
    pub expires_at: Option<DateTime<Utc>>,
    /// When it was last read.
    #[serde(with = "instant_serde::required")]
    pub last_usage: DateTime<Utc>,
    /// How many times it has been read.
    pub number_of_hits: u64,
    /// Implementation-specific extra information (clause 5.13).
    #[builder(default = Default::default())]
    pub extra_info: Option<JsonLiteral>,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::jsonld_context::{ContextKind, ContextMetadata};
    use andromeda_json_literal::JsonLiteral;
    use chrono::{TimeDelta, TimeZone, Utc};
    use serde_json::json;

    /// Clause 4.6.3 caps a `DateTime` at six fractional digits, so an instant
    /// read from a clock that resolves nanoseconds is written at the
    /// microsecond.
    #[test]
    fn context_metadata_writes_its_instants_at_the_microsecond() {
        let instant = Utc.with_ymd_and_hms(2026, 10, 19, 16, 27, 34).single().expect("a valid instant") + TimeDelta::nanoseconds(800_524_180);
        let metadata = ContextMetadata::builder()
            .serving_iri(None)
            .local_id("urn:ngsi-ld:Context:c1".parse().expect("a valid identifier"))
            .kind(ContextKind::ImplicitlyCreated)
            .created_at(instant)
            .expires_at(Some(instant))
            .last_usage(instant)
            .number_of_hits(0)
            .build();
        let written = serde_json::to_value(&metadata).expect("the metadata serialises");
        for member in ["createdAt", "expiresAt", "lastUsage"] {
            pretty_assertions::assert_eq!(written[member], json!("2026-10-19T16:27:34.800524Z"), "{member}");
        }
    }

    #[test]
    fn an_unknown_context_kind_label_is_a_parse_error_not_a_panic() {
        assert!("NotAKind".parse::<ContextKind>().is_err());
    }

    /// Clause 5.13.2 and 5.13.3: the posted `@context` document is held as the
    /// JSON it was posted as, and the implementation-specific `extraInfo`
    /// member is written back exactly as it was held.
    #[test]
    fn a_jsonld_context_round_trips_through_its_typed_model() {
        let document = r#"{"@context":{"Spot":"https://example.org/Spot","speed":{"@id":"https://example.org/speed","@type":"@json"}}}"#;
        let held: JsonLiteral = serde_json::from_slice(document.as_bytes()).expect("the document is admitted straight from its bytes");
        pretty_assertions::assert_eq!(
            serde_json::from_str::<serde_json::Value>(&serde_json::to_string(&held).expect("the document is written back")).expect("written JSON"),
            serde_json::from_str::<serde_json::Value>(document).expect("posted JSON")
        );
        let extra = r#"{"origin":{"mirrored":true},"weights":[1,2.5]}"#;
        let instant = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a valid instant");
        let metadata = ContextMetadata::builder()
            .serving_iri(None)
            .local_id("urn:ngsi-ld:Context:c1".parse().expect("a valid identifier"))
            .kind(ContextKind::Hosted)
            .created_at(instant)
            .last_usage(instant)
            .number_of_hits(3)
            .extra_info(Some(JsonLiteral::admit(extra).expect("an admissible literal")))
            .build();
        let written = serde_json::to_value(&metadata).expect("the metadata serialises");
        pretty_assertions::assert_eq!(
            written["extraInfo"],
            serde_json::from_str::<serde_json::Value>(extra).expect("parsed extra info")
        );
        pretty_assertions::assert_eq!(written["localId"], json!("urn:ngsi-ld:Context:c1"));
    }
}
