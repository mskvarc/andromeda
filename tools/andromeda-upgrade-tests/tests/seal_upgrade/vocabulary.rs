//! The names and documents a v1.0.0 database holds a seal as: the expanded
//! proof, the two seal members, and the `sub_attribute` elements that carry
//! them.

use serde_json::{Value, json};

/// The expanded name of the attribute a seal is stored under.
pub(crate) const SEAL_ATTRIBUTE: &str = "https://uri.etsi.org/ngsi-ld/ngsildproof";

/// The type the seals name.
pub(crate) const SEALED_TYPE: &str = "https://example.org/Store";

/// The entity the seals name.
pub(crate) const SEALED_ENTITY_ID: &str = "urn:ngsi-ld:Store:002";

/// The proof's `created`, as written.
pub(crate) const PROOF_CREATED: &str = "2025-01-27T21:02:24.500Z";

/// The expanded `entityIdSealed` member name.
const ENTITY_ID_SEALED: &str = "https://uri.etsi.org/ngsi-ld/entityIdSealed";

/// The expanded `entityTypeSealed` member name.
const ENTITY_TYPE_SEALED: &str = "https://uri.etsi.org/ngsi-ld/entityTypeSealed";

/// The proof's one member beside the six it must hold.
pub(crate) const FURTHER_MEMBER: &str = "https://w3id.org/security#challenge";

/// A complete expanded proof, under the W3C Data Integrity v2 terms.
pub(crate) fn proof_node() -> Value {
    json!({
        "@type": ["https://w3id.org/security#DataIntegrityProof"],
        "http://purl.org/dc/terms/created": [{"@type": "http://www.w3.org/2001/XMLSchema#dateTime", "@value": PROOF_CREATED}],
        "https://w3id.org/security#cryptosuite": [{"@type": "https://w3id.org/security#cryptosuiteString", "@value": "eddsa-rdfc-2022"}],
        "https://w3id.org/security#proofPurpose": [{"@id": "https://w3id.org/security#assertionMethod"}],
        "https://w3id.org/security#proofValue": [{"@type": "https://w3id.org/security#multibase", "@value": "z58DAdFfa9SkqZMVPxAQp"}],
        "https://w3id.org/security#verificationMethod": [{"@id": "https://example.org/keys/1#key-1"}],
        FURTHER_MEMBER: [{"@value": "challenge-1"}]
    })
}

/// [`proof_node`] without the member `member`.
pub(crate) fn proof_node_without(member: &str) -> Value {
    let mut proof = proof_node();
    if let Some(members) = proof.as_object_mut() {
        members.remove(member);
    }
    proof
}

/// The seals of an instance, with `type_values` as the expanded values of
/// `entityTypeSealed`.
pub(crate) fn seals(type_values: &Value) -> Value {
    json!({
        ENTITY_ID_SEALED: [{"@value": SEALED_ENTITY_ID}],
        ENTITY_TYPE_SEALED: type_values
    })
}

/// The seals of an instance naming [`SEALED_TYPE`] and nothing else.
pub(crate) fn complete_seals() -> Value {
    seals(&json!([{"@id": SEALED_TYPE}]))
}

/// A seal element: an `ngsildproof` Property whose object value is `proof`.
pub(crate) fn seal_element(parent: i32, seal_key: i32, proof: &Value, seals: &Value) -> Value {
    json!({
        "parent": parent,
        "attribute_name_key": seal_key,
        "attribute_kind": 1,
        "value_kind": 5,
        "value_object": proof,
        "proof_seals": seals
    })
}

/// A Property element holding the string `text`.
pub(crate) fn string_element(parent: i32, attribute_key: i32, text: &str) -> Value {
    json!({
        "parent": parent,
        "attribute_name_key": attribute_key,
        "attribute_kind": 1,
        "value_kind": 2,
        "value_string": text
    })
}
