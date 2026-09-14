//! Context Source identity configuration.

use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, uri::Uri};
use iri_rs::{IriBuf, iri};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use toml_comment::TomlComment;
use url::Url;

/// Identity returned by `/info/sourceIdentity` and used for distributed loop detection.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct InfoConfig {
    /// URI identifying this Context Source, per Table 5.2.40-1.
    #[toml_comment(inline)]
    pub source_id: Uri,
    /// Pseudonym appended to forwarded requests' `Via` headers.
    #[toml_comment(inline)]
    pub source_alias: ContextSourcePseudonym,
    /// Instance-specific information returned verbatim as
    /// `contextSourceExtras` (Table 5.2.40-1); raw JSON, never JSON-LD.
    pub source_extras: Option<Value>,
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
            // to the URN form, and no consumer observes the enum variant of this default.
            source_id: Uri::Iri(IriBuf::from(iri!("urn:ngsi-ld:ContextSourceIdentity:andromeda"))),
            source_alias: ContextSourcePseudonym::from_static("andromeda"),
            source_extras: None,
            advertised_url: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::InfoConfig;

    #[test]
    fn the_built_in_default_source_id_is_valid() {
        let config = InfoConfig::default();
        pretty_assertions::assert_eq!(config.source_id.as_str(), "urn:ngsi-ld:ContextSourceIdentity:andromeda");
        pretty_assertions::assert_eq!(config.source_alias.as_str(), "andromeda");
    }
}
