//! Context Source identity configuration.

use crate::schema::{source_alias::SourceAlias, source_extras::SourceExtras, source_identifier::SourceIdentifier};
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
    pub source_id: SourceIdentifier,
    /// Pseudonym appended to forwarded requests' `Via` headers.
    #[toml_comment(inline)]
    pub source_alias: SourceAlias,
    /// Instance-specific information returned verbatim as
    /// `contextSourceExtras` (Table 5.2.40-1), written as a table whose keys
    /// are reported as written; raw JSON, never JSON-LD, and never interpreted.
    pub source_extras: Option<SourceExtras>,
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
            source_id: SourceIdentifier::from_static(iri!("urn:ngsi-ld:ContextSourceIdentity:andromeda")),
            source_alias: SourceAlias::from_static("andromeda"),
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
    use crate::schema::identity::InfoConfig;

    #[test]
    fn the_built_in_default_names_the_andromeda_identity() {
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
            extras.to_json_text(),
            Ok(r#"{"region":"eu-west","nodes":[1,2],"nested":{"raw":true}}"#.to_owned())
        );
    }
}
