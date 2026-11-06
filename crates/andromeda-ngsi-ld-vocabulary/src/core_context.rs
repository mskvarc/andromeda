//! Every released NGSI-LD core `@context` document (clause 4.4), byte for byte
//! as ETSI serves it, each beside the URL ETSI serves it under.
//!
//! The v1.9 document is the file the vocabulary generator reads, so the
//! generated constants and the document the broker expands with are the same
//! bytes. Its URL is [`NGSI_LD_CORE_CONTEXT_URL`](crate::NGSI_LD_CORE_CONTEXT_URL).

use iri_rs::{Iri, iri};

/// Core Context Version 1.0 URL.
///
/// ETSI publishes Version 1.0 under the versionless file name and every later
/// release under a name carrying its version, so this URL names Version 1.0 and
/// no other release.
pub const NGSI_LD_CORE_CONTEXT_V10_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld");
/// Core Context Version 1.0 document, served under
/// [`NGSI_LD_CORE_CONTEXT_V10_URL`].
pub const NGSI_LD_CORE_CONTEXT_V10: &str = include_str!("../context/ngsi-ld-core-context-v1.0.jsonld");

/// Core Context v1.3 URL.
pub const NGSI_LD_CORE_CONTEXT_V13_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.3.jsonld");
/// Core Context v1.3 document, served under [`NGSI_LD_CORE_CONTEXT_V13_URL`].
pub const NGSI_LD_CORE_CONTEXT_V13: &str = include_str!("../context/ngsi-ld-core-context-v1.3.jsonld");

/// Core Context v1.4 URL.
pub const NGSI_LD_CORE_CONTEXT_V14_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.4.jsonld");
/// Core Context v1.4 document, served under [`NGSI_LD_CORE_CONTEXT_V14_URL`].
pub const NGSI_LD_CORE_CONTEXT_V14: &str = include_str!("../context/ngsi-ld-core-context-v1.4.jsonld");

/// Core Context v1.5 URL.
pub const NGSI_LD_CORE_CONTEXT_V15_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.5.jsonld");
/// Core Context v1.5 document, served under [`NGSI_LD_CORE_CONTEXT_V15_URL`].
pub const NGSI_LD_CORE_CONTEXT_V15: &str = include_str!("../context/ngsi-ld-core-context-v1.5.jsonld");

/// Core Context v1.6 URL.
pub const NGSI_LD_CORE_CONTEXT_V16_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld");
/// Core Context v1.6 document, served under [`NGSI_LD_CORE_CONTEXT_V16_URL`].
pub const NGSI_LD_CORE_CONTEXT_V16: &str = include_str!("../context/ngsi-ld-core-context-v1.6.jsonld");

/// Core Context v1.7 URL.
pub const NGSI_LD_CORE_CONTEXT_V17_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.7.jsonld");
/// Core Context v1.7 document, served under [`NGSI_LD_CORE_CONTEXT_V17_URL`].
pub const NGSI_LD_CORE_CONTEXT_V17: &str = include_str!("../context/ngsi-ld-core-context-v1.7.jsonld");

/// Core Context v1.8 URL.
pub const NGSI_LD_CORE_CONTEXT_V18_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld");
/// Core Context v1.8 document, served under [`NGSI_LD_CORE_CONTEXT_V18_URL`].
pub const NGSI_LD_CORE_CONTEXT_V18: &str = include_str!("../context/ngsi-ld-core-context-v1.8.jsonld");

/// Core Context v1.9 document, served under the v1.9 URL clause 4.4 names
/// ([`NGSI_LD_CORE_CONTEXT_URL`](crate::NGSI_LD_CORE_CONTEXT_URL), the one
/// constant for it). The vocabulary
/// generator reads the same file.
pub const NGSI_LD_CORE_CONTEXT_V19: &str = include_str!("../context/ngsi-ld-core-context-v1.9.jsonld");

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::core_context::{NGSI_LD_CORE_CONTEXT_V10, NGSI_LD_CORE_CONTEXT_V19};
    use sha2::{Digest, Sha256};

    /// ETSI serves Version 1.0 under the versionless URL as a 6,222-byte
    /// document with this SHA2-256 digest.
    #[test]
    fn the_version_1_0_document_is_the_one_etsi_serves() {
        pretty_assertions::assert_eq!(NGSI_LD_CORE_CONTEXT_V10.len(), 6222);
        pretty_assertions::assert_eq!(
            hex::encode(Sha256::digest(NGSI_LD_CORE_CONTEXT_V10.as_bytes())),
            "d751b5db4509071dc11b26342cfa36c05d18f85ea15554a4bac71a0705697905"
        );
    }

    /// Version 1.0 defines `name` and maps `title` into the NGSI-LD namespace,
    /// where v1.9 leaves `name` to its `@vocab` and maps `title` to Dublin Core.
    #[test]
    fn the_version_1_0_document_defines_terms_v1_9_does_not() {
        let document: serde_json::Value = serde_json::from_str(NGSI_LD_CORE_CONTEXT_V10).expect("the embedded core context is JSON");
        let terms = document["@context"].as_object().expect("the core context is a term map");
        pretty_assertions::assert_eq!(terms.get("name"), Some(&serde_json::json!("https://uri.etsi.org/ngsi-ld/name")));
        pretty_assertions::assert_eq!(terms.get("title"), Some(&serde_json::json!("https://uri.etsi.org/ngsi-ld/title")));

        let current: serde_json::Value = serde_json::from_str(NGSI_LD_CORE_CONTEXT_V19).expect("the embedded core context is JSON");
        let current_terms = current["@context"].as_object().expect("the core context is a term map");
        assert!(!current_terms.contains_key("name"));
        assert_ne!(current_terms.get("title"), terms.get("title"));
    }

    /// Annex B is normative ("the definition of the Core NGSI-LD @context
    /// which shall be supported by implementations"), but its printed listing
    /// is not self-consistent with the clauses it serves, so the served payload
    /// deviates from it in four places, each recorded here so a later reader
    /// finds the reasoning rather than an unexplained difference.
    ///
    /// Six terms the spec body defines are absent from the Annex B listing:
    /// `EntityMap` and `entityMapLifetime` (clause 5.2.39 / 5.2.23),
    /// `splitEntities` (clause 5.2.23), `lastN` (clause 6.3.10),
    /// `ngsildConformance` (clause 5.2.12) and `jsonldContext` (clause 5.13).
    /// A broker that omitted them could not compact those members at all.
    ///
    /// `entityMap` carries `"@container": "@index"` here where Annex B gives a
    /// bare IRI, because clause 4.5.25 calls the member "a JSON-LD index map
    /// holding a unique list of entity ids", which is what that container
    /// encodes.
    ///
    /// `attributeCount` and `attributeDetails` map to `ngsi-ld:` here where
    /// Annex B maps each to its own name. They are the only two self-mappings
    /// in the whole listing, and the core context declares no `@vocab` for such
    /// a term to resolve against, so the printed form is a typo rather than a
    /// rule.
    ///
    /// The test pins the deviations rather than the whole document: it fails if
    /// one of the six terms is dropped or one of the three definitions drifts,
    /// which is the way this file could silently stop interoperating.
    #[test]
    fn the_served_core_context_carries_the_documented_deviations_from_annex_b() {
        let document: serde_json::Value = serde_json::from_str(NGSI_LD_CORE_CONTEXT_V19).expect("the embedded core context is JSON");
        let terms = document["@context"].as_object().expect("the core context is a term map");

        for absent_from_annex_b in ["EntityMap", "entityMapLifetime", "jsonldContext", "lastN", "ngsildConformance", "splitEntities"] {
            assert!(terms.contains_key(absent_from_annex_b), "{absent_from_annex_b} is defined by the spec body");
        }

        pretty_assertions::assert_eq!(
            terms.get("entityMap"),
            Some(&serde_json::json!({"@id": "ngsi-ld:hasEntityMap", "@container": "@index"})),
            "clause 4.5.25 makes entityMap a JSON-LD index map"
        );
        pretty_assertions::assert_eq!(terms.get("attributeCount"), Some(&serde_json::json!("ngsi-ld:attributeCount")));
        pretty_assertions::assert_eq!(terms.get("attributeDetails"), Some(&serde_json::json!("ngsi-ld:attributeDetails")));
    }

    /// The seals of clause 4.5.2.2 are reachable only inside `ngsildproof`'s own
    /// scoped context, which is what makes a seal on any other attribute an
    /// undefined term rather than a valid member. Annex B declares them the same
    /// way, and flattening them to the top level would silently admit the
    /// spelling clauses 4.5.3.2 and 4.5.3.3 prohibit outright.
    #[test]
    fn the_proof_seals_stay_scoped_to_the_ngsildproof_term() {
        let document: serde_json::Value = serde_json::from_str(NGSI_LD_CORE_CONTEXT_V19).expect("the embedded core context is JSON");
        let terms = document["@context"].as_object().expect("the core context is a term map");

        assert!(!terms.contains_key("entityIdSealed"), "a top-level seal would apply to every attribute");
        assert!(!terms.contains_key("entityTypeSealed"), "a top-level seal would apply to every attribute");

        let scoped = terms["ngsildproof"]["@context"].as_array().expect("ngsildproof carries a scoped context");
        let declarations = scoped[0].as_object().expect("the scoped context declares the seals");
        assert!(declarations.contains_key("entityIdSealed"));
        assert!(declarations.contains_key("entityTypeSealed"));
    }
}
