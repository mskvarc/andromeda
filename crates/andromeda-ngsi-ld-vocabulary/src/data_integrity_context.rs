//! The W3C Data Integrity v2 `@context` document, which the v1.9 Core @context
//! names as the scoped context of the `ngsildproof` term (clause 4.5.2.2).

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
pub const DATA_INTEGRITY_CONTEXT: &str = include_str!("../context/data-integrity-v2.jsonld");

#[cfg(test)]
mod tests {
    use crate::data_integrity_context::DATA_INTEGRITY_CONTEXT;
    use sha2::{Digest, Sha256};

    /// W3C Verifiable Credential Data Integrity 1.0 section 2.4 lists this
    /// SHA2-256 digest for `https://w3id.org/security/data-integrity/v2`.
    const PUBLISHED_DATA_INTEGRITY_DIGEST: &str = "67f21e6e33a6c14e5ccfd2fc7865f7474fb71a04af7e94136cb399dfac8ae8f4";

    #[test]
    fn the_data_integrity_context_has_the_digest_the_w3c_publishes() {
        pretty_assertions::assert_eq!(hex::encode(Sha256::digest(DATA_INTEGRITY_CONTEXT.as_bytes())), PUBLISHED_DATA_INTEGRITY_DIGEST);
    }

    #[test]
    fn the_data_integrity_context_defines_the_w3c_proof_terms() {
        let document: serde_json::Value = serde_json::from_str(DATA_INTEGRITY_CONTEXT).expect("the document is JSON");
        let terms = document["@context"].as_object().expect("the document is a term map");
        pretty_assertions::assert_eq!(terms["proof"]["@id"], "https://w3id.org/security#proof");
        let proof_terms = terms["DataIntegrityProof"]["@context"].as_object().expect("the proof type carries its terms");
        for term in ["cryptosuite", "proofValue", "verificationMethod", "proofPurpose", "created"] {
            assert!(proof_terms.contains_key(term), "{term} is a Data Integrity proof term");
        }
    }
}
