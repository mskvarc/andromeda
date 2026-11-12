//! The `[federation.identity]` table: the identity this broker reports as a
//! Context Source.

use crate::schema::{source_alias::SourceAlias, source_extras::SourceExtras, source_identifier::SourceIdentifier};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The identity returned by `/info/sourceIdentity` and used for distributed
/// loop detection (Table 5.2.40-1).
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Default, PartialEq)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "The identity the broker reports at `/ngsi-ld/v1/info/sourceIdentity` and uses to detect distributed forwarding loops.")]
pub struct IdentityConfig {
    /// The URI identifying this Context Source. Left out, it is
    /// `urn:ngsi-ld:ContextSourceIdentity:` followed by the alias in effect,
    /// every character but letters, digits and `-._~!$&'*+` percent-encoded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "The `id` the broker reports at `/ngsi-ld/v1/info/sourceIdentity`. Must be an IRI. Left out, it is `urn:ngsi-ld:ContextSourceIdentity:` followed by the alias in effect, as `federation.identity.source_alias` describes, with every character of the alias other than letters, digits and `-._~!$&'*+` percent-encoded, so the alias `broker|a` gives `urn:ngsi-ld:ContextSourceIdentity:broker%7Ca`."
    )]
    pub source_id: Option<SourceIdentifier>,
    /// The pseudonym appended to forwarded requests' `Via` fields. Left out,
    /// the host of `server.public_url`, or the machine's host name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "The name the broker adds to the `Via` header of every request it forwards. A request whose `Via` already holds this name is treated as a forwarding loop, so every broker in a federation needs its own. Must be an HTTP token, as a `Via` entry requires. Left out, it is the host of `server.public_url` without its port, such as `broker.example.org`, and without a `server.public_url`, or with an IPv6 one, the machine's host name. The broker refuses to start when neither gives a token. Set it when two brokers share a host name."
    )]
    pub source_alias: Option<SourceAlias>,
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

#[cfg(test)]
mod tests {
    use crate::schema::federation::identity::IdentityConfig;

    #[test]
    fn the_built_in_default_names_no_identity() {
        let config = IdentityConfig::default();
        pretty_assertions::assert_eq!(config.source_id, None);
        pretty_assertions::assert_eq!(config.source_alias, None);
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
