//! The `@context` documents compiled into the broker.
//!
//! Every released NGSI-LD core context (clause 4.4) is embedded, and so is the
//! one document the v1.9 Core @context names itself: the W3C Data Integrity v2
//! context, the scoped context of the `ngsildproof` term (clause 4.5.2.2).
//! Embedding both means the Core @context expands completely without a
//! network, so a proof member written with a W3C term expands to the same IRI
//! whether or not the broker can reach the W3C.
//!
//! An embedded document is resolved from memory and never fetched. It is no
//! clause 5.13 entry: the @context management operations neither list nor
//! serve it, the durable context cache keeps no record of it, and an operator
//! cannot preload a replacement for it.

use crate::core_context::{NGSI_LD_CORE_CONTEXT_MAP, embedded_core_context, is_ngsi_ld_core_context_url};
use iri_rs::{Iri, iri};

/// The URL the W3C publishes the Data Integrity v2 context under, which the
/// v1.9 Core @context names as part of the scoped context of `ngsildproof`.
pub const DATA_INTEGRITY_CONTEXT_URL: Iri<&'static str> = iri!("https://w3id.org/security/data-integrity/v2");

/// The Data Integrity v2 context, byte for byte as the W3C serves it at the
/// address [`DATA_INTEGRITY_CONTEXT_URL`] redirects to.
///
/// W3C Verifiable Credential Data Integrity 1.0 section 2.4 publishes the
/// document's SHA2-256 digest and lets a processor treat the URL as already
/// resolved when the document matches it.
pub const DATA_INTEGRITY_CONTEXT: &str = include_str!("context/data-integrity-v2.jsonld");

/// Whether `url` names a document compiled into the broker.
#[must_use]
pub fn is_embedded_context_url(url: Iri<&str>) -> bool {
    is_ngsi_ld_core_context_url(url) || url == DATA_INTEGRITY_CONTEXT_URL
}

/// The document compiled into the broker under `url`, if there is one.
#[must_use]
pub fn embedded_context(url: Iri<&str>) -> Option<&'static str> {
    embedded_core_context(url).or_else(|| (url == DATA_INTEGRITY_CONTEXT_URL).then_some(DATA_INTEGRITY_CONTEXT))
}

/// Every URL a document is compiled into the broker under.
pub fn embedded_context_urls() -> impl Iterator<Item = Iri<&'static str>> {
    NGSI_LD_CORE_CONTEXT_MAP.iter().map(|(url, _payload)| *url).chain([DATA_INTEGRITY_CONTEXT_URL])
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        core_context::NGSI_LD_CORE_CONTEXT_URL_LIST,
        embedded_context::{DATA_INTEGRITY_CONTEXT, DATA_INTEGRITY_CONTEXT_URL, embedded_context, embedded_context_urls, is_embedded_context_url},
    };
    use iri_rs::iri;
    use sha2::{Digest, Sha256};

    /// W3C Verifiable Credential Data Integrity 1.0 section 2.4 lists this
    /// SHA2-256 digest for `https://w3id.org/security/data-integrity/v2`.
    const PUBLISHED_DATA_INTEGRITY_DIGEST: &str = "67f21e6e33a6c14e5ccfd2fc7865f7474fb71a04af7e94136cb399dfac8ae8f4";

    #[test]
    fn the_embedded_data_integrity_context_has_the_digest_the_w3c_publishes() {
        pretty_assertions::assert_eq!(hex::encode(Sha256::digest(DATA_INTEGRITY_CONTEXT.as_bytes())), PUBLISHED_DATA_INTEGRITY_DIGEST);
    }

    #[test]
    fn the_embedded_data_integrity_context_defines_the_w3c_proof_terms() {
        let document: serde_json::Value = serde_json::from_str(DATA_INTEGRITY_CONTEXT).expect("the embedded document is JSON");
        let terms = document["@context"].as_object().expect("the document is a term map");
        pretty_assertions::assert_eq!(terms["proof"]["@id"], "https://w3id.org/security#proof");
        let proof_terms = terms["DataIntegrityProof"]["@context"].as_object().expect("the proof type carries its terms");
        for term in ["cryptosuite", "proofValue", "verificationMethod", "proofPurpose", "created"] {
            assert!(proof_terms.contains_key(term), "{term} is a Data Integrity proof term");
        }
    }

    #[test]
    fn every_core_context_and_the_data_integrity_context_are_embedded() {
        for url in NGSI_LD_CORE_CONTEXT_URL_LIST.into_iter().chain([DATA_INTEGRITY_CONTEXT_URL]) {
            assert!(is_embedded_context_url(url), "{url} is embedded");
            assert!(embedded_context(url).is_some(), "{url} has a payload");
            assert!(embedded_context_urls().any(|embedded| embedded == url), "{url} is registered");
        }
    }

    #[test]
    fn a_context_the_broker_does_not_compile_in_is_not_embedded() {
        let url = iri!("https://w3id.org/security/multikey/v1");
        assert!(!is_embedded_context_url(url));
        assert!(embedded_context(url).is_none());
    }
}
