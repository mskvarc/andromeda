//! Every released NGSI-LD core `@context` document (clause 4.4), byte for byte
//! as ETSI serves it, each beside the URL ETSI serves it under and the
//! SHA2-256 digest of the bytes ETSI served when the copy was taken.
//!
//! The v1.9 document is the file the vocabulary generator reads, so the
//! generated constants and the document the broker expands with are the same
//! bytes. Its URL is [`NGSI_LD_CORE_CONTEXT_URL`].

use crate::{NGSI_LD_CORE_CONTEXT_URL, core_context_release::CoreContextRelease};
use hex_literal::hex;
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
/// SHA2-256 digest of the Version 1.0 document ETSI serves.
pub const NGSI_LD_CORE_CONTEXT_V10_DIGEST: [u8; 32] = hex!("d751b5db4509071dc11b26342cfa36c05d18f85ea15554a4bac71a0705697905");

/// Core Context v1.3 URL.
pub const NGSI_LD_CORE_CONTEXT_V13_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.3.jsonld");
/// Core Context v1.3 document, served under [`NGSI_LD_CORE_CONTEXT_V13_URL`].
pub const NGSI_LD_CORE_CONTEXT_V13: &str = include_str!("../context/ngsi-ld-core-context-v1.3.jsonld");
/// SHA2-256 digest of the v1.3 document ETSI serves.
pub const NGSI_LD_CORE_CONTEXT_V13_DIGEST: [u8; 32] = hex!("af3a9bf2e738f9807fa2970a6b4e7063382a9df1c684f8f80119450e45c8a67f");

/// Core Context v1.4 URL.
pub const NGSI_LD_CORE_CONTEXT_V14_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.4.jsonld");
/// Core Context v1.4 document, served under [`NGSI_LD_CORE_CONTEXT_V14_URL`].
pub const NGSI_LD_CORE_CONTEXT_V14: &str = include_str!("../context/ngsi-ld-core-context-v1.4.jsonld");
/// SHA2-256 digest of the v1.4 document ETSI serves.
pub const NGSI_LD_CORE_CONTEXT_V14_DIGEST: [u8; 32] = hex!("2c1e39e22eac7d16bcce1200de27d1cf0e634d5855e97e2f4779dc6f962efc86");

/// Core Context v1.5 URL.
pub const NGSI_LD_CORE_CONTEXT_V15_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.5.jsonld");
/// Core Context v1.5 document, served under [`NGSI_LD_CORE_CONTEXT_V15_URL`].
pub const NGSI_LD_CORE_CONTEXT_V15: &str = include_str!("../context/ngsi-ld-core-context-v1.5.jsonld");
/// SHA2-256 digest of the v1.5 document ETSI serves.
pub const NGSI_LD_CORE_CONTEXT_V15_DIGEST: [u8; 32] = hex!("5d27a01b817b126f5d823e508bd235e589c564c1ec8cd4fe17d43661b9c9f69e");

/// Core Context v1.6 URL.
pub const NGSI_LD_CORE_CONTEXT_V16_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld");
/// Core Context v1.6 document, served under [`NGSI_LD_CORE_CONTEXT_V16_URL`].
pub const NGSI_LD_CORE_CONTEXT_V16: &str = include_str!("../context/ngsi-ld-core-context-v1.6.jsonld");
/// SHA2-256 digest of the v1.6 document ETSI serves.
pub const NGSI_LD_CORE_CONTEXT_V16_DIGEST: [u8; 32] = hex!("b43a8fbdf3e2bf1f889d75cfe4f31239fe72e02dab20967f08ff1c7c69b6eb75");

/// Core Context v1.7 URL.
pub const NGSI_LD_CORE_CONTEXT_V17_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.7.jsonld");
/// Core Context v1.7 document, served under [`NGSI_LD_CORE_CONTEXT_V17_URL`].
pub const NGSI_LD_CORE_CONTEXT_V17: &str = include_str!("../context/ngsi-ld-core-context-v1.7.jsonld");
/// SHA2-256 digest of the v1.7 document ETSI serves.
pub const NGSI_LD_CORE_CONTEXT_V17_DIGEST: [u8; 32] = hex!("895e6327be27513c57bd7f7c0216bc970c5cee051d1b3decb6a54e1e614fa267");

/// Core Context v1.8 URL.
pub const NGSI_LD_CORE_CONTEXT_V18_URL: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld");
/// Core Context v1.8 document, served under [`NGSI_LD_CORE_CONTEXT_V18_URL`].
pub const NGSI_LD_CORE_CONTEXT_V18: &str = include_str!("../context/ngsi-ld-core-context-v1.8.jsonld");
/// SHA2-256 digest of the v1.8 document ETSI serves.
pub const NGSI_LD_CORE_CONTEXT_V18_DIGEST: [u8; 32] = hex!("4ac48e7d00cb0378546570ab0dbc46b3717da78ca4cb0fac98b0e32665046cca");

/// Core Context v1.9 document, served under the v1.9 URL clause 4.4 names
/// ([`NGSI_LD_CORE_CONTEXT_URL`], the one constant for it). The vocabulary
/// generator reads the same file.
pub const NGSI_LD_CORE_CONTEXT_V19: &str = include_str!("../context/ngsi-ld-core-context-v1.9.jsonld");
/// SHA2-256 digest of the v1.9 document ETSI serves.
pub const NGSI_LD_CORE_CONTEXT_V19_DIGEST: [u8; 32] = hex!("b076a0ea02a6824acc763bb03c50d854a0820df9ecba6bc37ee5548c79dd34c4");

/// Every core context release compiled into the broker, oldest first.
pub const NGSI_LD_CORE_CONTEXT_RELEASES: [CoreContextRelease; 8] = [
    CoreContextRelease::new(NGSI_LD_CORE_CONTEXT_V10_URL, NGSI_LD_CORE_CONTEXT_V10, NGSI_LD_CORE_CONTEXT_V10_DIGEST),
    CoreContextRelease::new(NGSI_LD_CORE_CONTEXT_V13_URL, NGSI_LD_CORE_CONTEXT_V13, NGSI_LD_CORE_CONTEXT_V13_DIGEST),
    CoreContextRelease::new(NGSI_LD_CORE_CONTEXT_V14_URL, NGSI_LD_CORE_CONTEXT_V14, NGSI_LD_CORE_CONTEXT_V14_DIGEST),
    CoreContextRelease::new(NGSI_LD_CORE_CONTEXT_V15_URL, NGSI_LD_CORE_CONTEXT_V15, NGSI_LD_CORE_CONTEXT_V15_DIGEST),
    CoreContextRelease::new(NGSI_LD_CORE_CONTEXT_V16_URL, NGSI_LD_CORE_CONTEXT_V16, NGSI_LD_CORE_CONTEXT_V16_DIGEST),
    CoreContextRelease::new(NGSI_LD_CORE_CONTEXT_V17_URL, NGSI_LD_CORE_CONTEXT_V17, NGSI_LD_CORE_CONTEXT_V17_DIGEST),
    CoreContextRelease::new(NGSI_LD_CORE_CONTEXT_V18_URL, NGSI_LD_CORE_CONTEXT_V18, NGSI_LD_CORE_CONTEXT_V18_DIGEST),
    CoreContextRelease::new(NGSI_LD_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_V19, NGSI_LD_CORE_CONTEXT_V19_DIGEST),
];

#[cfg(test)]
mod tests {
    use crate::core_context::{NGSI_LD_CORE_CONTEXT_RELEASES, NGSI_LD_CORE_CONTEXT_V10, NGSI_LD_CORE_CONTEXT_V19};
    use iri_rs::Iri;
    use reqwest::Client;
    use sha2::{Digest, Sha256};
    use std::{fs, path::PathBuf};

    #[test]
    fn every_embedded_release_has_its_recorded_digest() {
        let mismatches: Vec<String> = NGSI_LD_CORE_CONTEXT_RELEASES
            .iter()
            .filter_map(|release| {
                let embedded: [u8; 32] = Sha256::digest(release.document().as_bytes()).into();
                (embedded != release.digest()).then(|| {
                    format!(
                        "{}: recorded {}, embedded {}",
                        release.url(),
                        hex::encode(release.digest()),
                        hex::encode(embedded)
                    )
                })
            })
            .collect();
        assert!(
            mismatches.is_empty(),
            "embedded releases differ from their recorded digest:\n{}",
            mismatches.join("\n")
        );
    }

    /// A core context document in the context directory that no release entry
    /// embeds has no recorded digest.
    #[test]
    fn every_core_context_document_is_a_release_with_a_recorded_digest() {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("context");
        let documents: Vec<(String, String)> = fs::read_dir(&directory)
            .expect("the context directory is readable")
            .map(|entry| entry.expect("the context directory lists its entries").path())
            .filter_map(|path| {
                let name = path.file_name()?.to_str()?.to_owned();
                (name.starts_with("ngsi-ld-core-context") && name.ends_with(".jsonld"))
                    .then(|| (name, fs::read_to_string(&path).expect("a core context document is UTF-8")))
            })
            .collect();
        let unrecorded: Vec<&str> = documents
            .iter()
            .filter(|(_name, text)| !NGSI_LD_CORE_CONTEXT_RELEASES.iter().any(|release| release.document() == text))
            .map(|(name, _text)| name.as_str())
            .collect();
        assert!(unrecorded.is_empty(), "core context documents with no recorded digest: {unrecorded:?}");
        pretty_assertions::assert_eq!(documents.len(), NGSI_LD_CORE_CONTEXT_RELEASES.len(), "every release is its own document");
    }

    /// ETSI replaces a release in place and publishes no digests, so the only
    /// way to notice an upstream change is to fetch what it serves now.
    #[tokio::test]
    async fn every_embedded_release_is_the_document_etsi_serves() {
        let client = Client::new();
        let mut differing = Vec::new();
        for release in NGSI_LD_CORE_CONTEXT_RELEASES {
            match served_digest(&client, release.url()).await {
                Ok(served) if served == release.digest() => {}
                Ok(served) => differing.push(format!(
                    "{}: recorded {}, ETSI serves {}",
                    release.url(),
                    hex::encode(release.digest()),
                    hex::encode(served)
                )),
                Err(error) => differing.push(format!("{}: download failed: {error}", release.url())),
            }
        }
        assert!(differing.is_empty(), "releases ETSI serves differently:\n{}", differing.join("\n"));
    }

    /// The SHA2-256 digest of the document ETSI serves under `url`.
    async fn served_digest(client: &Client, url: Iri<&'static str>) -> Result<[u8; 32], reqwest::Error> {
        let served = client.get(url.as_str()).send().await?.error_for_status()?.bytes().await?;
        Ok(Sha256::digest(&served).into())
    }

    #[test]
    fn every_release_is_served_under_its_own_url() {
        for release in NGSI_LD_CORE_CONTEXT_RELEASES {
            let sharing = NGSI_LD_CORE_CONTEXT_RELEASES.iter().filter(|other| other.url() == release.url()).count();
            pretty_assertions::assert_eq!(sharing, 1, "{} names more than one release", release.url());
        }
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
