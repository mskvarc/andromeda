//! The identity a Context Source answers `/info` with (clause 6.33.3.1).

use andromeda_ngsi_ld_lexicon::{constant_string, context_source_pseudonym::ContextSourcePseudonym, uri::Uri};
use serde::Serialize;

constant_string!(ContextSourceIdentityType, "ContextSourceIdentity");

/// The response body of the Context Source identity endpoint.
///
/// Table 5.2.40-1 separates the source's URI identifier from the pseudonym it
/// places in `Via` headers for loop detection.
#[derive(Debug, Serialize)]
pub struct ContextSourceIdentity {
    /// The identifier of this context source.
    pub id: Uri,
    /// Always `"ContextSourceIdentity"`.
    #[serde(rename = "type")]
    pub type_: ContextSourceIdentityType,
    /// The pseudonym used to identify this source in a `Via` chain.
    #[serde(rename = "contextSourceAlias")]
    pub context_source_alias: ContextSourcePseudonym,
}

#[cfg(test)]
mod tests {
    use crate::context_source_identity::{ContextSourceIdentity, ContextSourceIdentityType};

    #[test]
    fn the_identity_renders_its_type_under_the_spec_member_name() {
        let rendered = serde_json::to_value(ContextSourceIdentity {
            id: "urn:ngsi-ld:ContextSourceIdentity:andromeda".parse().expect("valid context source id"),
            type_: ContextSourceIdentityType,
            context_source_alias: "andromeda".parse().expect("valid context source alias"),
        })
        .expect("serialises");
        pretty_assertions::assert_eq!(rendered["id"], "urn:ngsi-ld:ContextSourceIdentity:andromeda");
        pretty_assertions::assert_eq!(rendered["type"], "ContextSourceIdentity");
        pretty_assertions::assert_eq!(rendered["contextSourceAlias"], "andromeda");
        assert!(rendered.get("type_").is_none());
    }
}
