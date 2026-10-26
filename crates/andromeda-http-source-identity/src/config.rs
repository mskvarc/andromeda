//! Context Source identity configuration.

use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
use iri_rs::iri;
use serde::{Deserialize, Serialize};
use toml_comment::TomlComment;
use url::Url;

/// Identity returned by `/info/sourceIdentity` and used for distributed loop detection.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct InfoConfig {
    /// URI identifying this Context Source, per Table 5.2.40-1.
    #[toml_comment(inline)]
    pub source_id: Identifier,
    /// Pseudonym appended to forwarded requests' `Via` headers.
    #[toml_comment(inline)]
    pub source_alias: ContextSourcePseudonym,
    /// Instance-specific information returned verbatim as
    /// `contextSourceExtras` (Table 5.2.40-1); raw JSON, never JSON-LD, held as
    /// admitted data and never interpreted.
    pub source_extras: Option<JsonLiteral>,
    /// The origin peers can reach this broker at, such as `https://broker-a:1056`.
    ///
    /// Subscription forwarding (clauses 5.8.1.4 and 5.8.6) names this broker as
    /// the notification endpoint of every subscription it places at a context
    /// source; without it the broker cannot receive those notifications, and
    /// forwarding stays off.
    pub advertised_url: Option<Url>,
}

impl Default for InfoConfig {
    fn default() -> InfoConfig {
        InfoConfig {
            // Compile-time validated, so the built-in default needs no runtime parse or panic.
            // Held verbatim as an IRI; a loaded configuration deserializes the same string back
            // to the URN form, and no consumer observes the form of this default.
            source_id: Identifier::from_static(iri!("urn:ngsi-ld:ContextSourceIdentity:andromeda")),
            source_alias: ContextSourcePseudonym::from_static("andromeda"),
            source_extras: None,
            advertised_url: None,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::config::InfoConfig;
    use andromeda_json_literal::JsonLiteral;

    #[test]
    fn the_built_in_default_source_id_is_valid() {
        let config = InfoConfig::default();
        pretty_assertions::assert_eq!(config.source_id.as_str(), "urn:ngsi-ld:ContextSourceIdentity:andromeda");
        pretty_assertions::assert_eq!(config.source_alias.as_str(), "andromeda");
    }

    /// Table 5.2.40-1: the extras are the operator's raw JSON, read from the
    /// configuration as the data it spells.
    #[test]
    fn the_source_extras_are_read_from_the_configuration_as_data() {
        let config: InfoConfig = toml::from_str(
            r#"
            source_id = "urn:ngsi-ld:ContextSourceIdentity:node"
            source_alias = "node"
            [source_extras]
            region = "eu-west"
            nodes = [1, 2]
            [source_extras.nested]
            raw = true
            "#,
        )
        .expect("a configuration with extras");
        let extras = config.source_extras.expect("extras are configured");
        pretty_assertions::assert_eq!(
            extras,
            JsonLiteral::admit(r#"{"region": "eu-west", "nodes": [1, 2], "nested": {"raw": true}}"#).expect("a JSON literal")
        );
    }
}
