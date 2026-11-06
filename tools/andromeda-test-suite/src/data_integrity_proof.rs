//! A W3C Data Integrity proof written with the terms of the W3C Data Integrity v2 context, the value an `ngsildproof`
//! carries (clause 4.5.2.2).

use ngsi_ld::model::Value;
use serde_json::Value as Json;

/// The proof's members, each written with a term the W3C Data Integrity v2 context defines inside
/// `DataIntegrityProof`.
const MEMBERS: [(&str, &str); 6] = [
    ("type", "DataIntegrityProof"),
    ("created", "2025-01-27T21:02:24Z"),
    ("verificationMethod", "https://example.edu/issuers/565049#z6MkwXG2WjeQnN"),
    ("cryptosuite", "eddsa-rdfc-2022"),
    ("proofPurpose", "assertionMethod"),
    ("proofValue", "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx"),
];

/// The proof, as the value of an `ngsildproof` a write sends.
#[must_use]
pub fn proof_value() -> Value {
    Value::Object(MEMBERS.iter().map(|(name, member)| ((*name).to_owned(), Value::from(*member))).collect())
}

/// The proof as JSON, which a broker answers it as when it compacts it with the terms it was written with.
#[must_use]
pub fn proof_json() -> Json {
    Json::Object(MEMBERS.iter().map(|(name, member)| ((*name).to_owned(), Json::from(*member))).collect())
}

/// A user `@context` defining `DataIntegrityProof` and every other term the proof is written with under `vocabulary`.
///
/// Inside `ngsildproof` the W3C Data Integrity v2 context the Core @context names defines the same terms, and a
/// scoped context overrides what the user `@context` defines, so a proof written under this `@context` still expands to
/// the W3C IRIs; were that context empty, it would expand under `vocabulary` instead.
#[must_use]
pub fn shadowing_terms(vocabulary: &str) -> String {
    let terms = MEMBERS
        .iter()
        .map(|(name, member)| if *name == "type" { *member } else { *name })
        .map(|term| (term.to_owned(), Json::from(format!("{vocabulary}{term}"))));
    Json::Object(terms.collect()).to_string()
}
