//! The `[federation.identity]` table: the identity this broker reports as a
//! Context Source.

use crate::schema::{source_alias::SourceAlias, source_extras::SourceExtras, source_identifier::SourceIdentifier};
use iri_rs::iri;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The identity returned by `/info/sourceIdentity` and used for distributed
/// loop detection (Table 5.2.40-1).
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
#[schemars(description = "The identity the broker reports at `/ngsi-ld/v1/info/sourceIdentity` and uses to detect distributed forwarding loops.")]
pub struct IdentityConfig {
    /// The URI identifying this Context Source.
    #[schemars(description = "The `id` the broker reports at `/ngsi-ld/v1/info/sourceIdentity`. Must be an IRI.")]
    pub source_id: SourceIdentifier,
    /// The pseudonym appended to forwarded requests' `Via` fields.
    #[schemars(
        description = "The name the broker adds to the `Via` header of every request it forwards. A request whose `Via` already holds this name is treated as a forwarding loop, so every broker in a federation needs its own. Must be an HTTP token, as a `Via` entry requires."
    )]
    pub source_alias: SourceAlias,
    /// Instance-specific information reported verbatim as
    /// `contextSourceExtras`, written as a table whose keys are reported as
    /// written. Raw JSON, never JSON-LD, never interpreted. Set in the file
    /// only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Instance-specific information the broker reports verbatim as the NGSI-LD `contextSourceExtras` member at `/ngsi-ld/v1/info/sourceIdentity`, with the enabled extensions listed beside it. Its keys are reported exactly as written and none is reserved, since the member is raw JSON that is never interpreted. Set in the file only. A date-time, `inf` or `nan`, which JSON cannot spell, is refused when the broker starts. Left out, `contextSourceExtras` lists only the enabled extensions, and is absent when none is enabled."
    )]
    pub extras: Option<SourceExtras>,
}

impl Default for IdentityConfig {
    fn default() -> IdentityConfig {
        IdentityConfig {
            // Compile-time validated, so the built-in default needs no runtime parse or panic.
            source_id: SourceIdentifier::from_static(iri!("urn:ngsi-ld:ContextSourceIdentity:andromeda")),
            source_alias: SourceAlias::from_static("andromeda"),
            extras: None,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::federation::identity::IdentityConfig;

    #[test]
    fn the_built_in_default_names_the_andromeda_identity() {
        let config = IdentityConfig::default();
        pretty_assertions::assert_eq!(config.source_id.as_str(), "urn:ngsi-ld:ContextSourceIdentity:andromeda");
        pretty_assertions::assert_eq!(config.source_alias.as_str(), "andromeda");
    }

    /// Table 5.2.40-1: the extras are the operator's raw JSON, read from the
    /// configuration as the data it spells, whatever its keys.
    #[test]
    fn the_extras_are_read_from_the_configuration_as_data() {
        let config: IdentityConfig = toml::from_str(
            r#"
            source_id = "urn:ngsi-ld:ContextSourceIdentity:node"
            source_alias = "node"
            [extras]
            id = "kept-as-written"
            region = "eu-west"
            nodes = [1, 2]
            [extras.nested]
            raw = true
            "#,
        )
        .expect("a configuration with extras");
        let extras = config.extras.expect("extras are configured");
        pretty_assertions::assert_eq!(
            extras.to_json_text(),
            Ok(r#"{"id":"kept-as-written","region":"eu-west","nodes":[1,2],"nested":{"raw":true}}"#.to_owned())
        );
    }
}
