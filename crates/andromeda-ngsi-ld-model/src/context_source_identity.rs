//! The identity a Context Source answers `/info` with (clause 6.36.3.1).

use serde::Serialize;

/// The response body of the Context Source identity endpoint.
///
/// Clause 6.36.3.1 fixes both members: the identifier this source is known by
/// when it appears in a `Via` header or a registration, and a `type` that is
/// always `"ContextSourceIdentity"`.
#[derive(Debug, Serialize)]
pub struct ContextSourceIdentity {
    /// The identifier of this context source.
    pub id: String,
    /// Always `"ContextSourceIdentity"`.
    #[serde(rename = "type")]
    pub type_: &'static str,
}

#[cfg(test)]
mod tests {
    use crate::context_source_identity::ContextSourceIdentity;

    #[test]
    fn the_identity_renders_its_type_under_the_spec_member_name() {
        let rendered = serde_json::to_value(ContextSourceIdentity {
            id: "urn:ngsi-ld:ContextSourceIdentity:andromeda".to_string(),
            type_: "ContextSourceIdentity",
        })
        .expect("serialises");
        pretty_assertions::assert_eq!(rendered["id"], "urn:ngsi-ld:ContextSourceIdentity:andromeda");
        pretty_assertions::assert_eq!(rendered["type"], "ContextSourceIdentity");
        assert!(rendered.get("type_").is_none());
    }
}
