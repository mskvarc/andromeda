//! RDFC-1.0 canonicalisation for the `*-rdfc-*` cryptosuites.
//!
//! Pipeline:
//!   1. Construct the expanded JSON-LD form of the unsigned atomic
//!      entity by re-parsing the unsigned map as an [`Entity`] and
//!      calling its `JsonLdExpandable` impl.
//!   2. Walk the expanded JSON-LD into oxrdf [`Quad`]s.
//!   3. Canonicalise via [`rdf_canon::canonicalize`] (RDFC-1.0).
//!   4. SHA-256 the canonical N-Quads UTF-8 bytes.
//!
//! The proof configuration (`DataIntegrityProof` with `proofValue`
//! stripped) is hashed the same way, using the Data Integrity vocab
//! IRIs expanded by hand.
//!
//! The walker is intentionally narrow: it handles only the expanded
//! JSON-LD shapes actually produced by NGSI-LD entities and the
//! security vocab. It does not implement @list, @index, or @graph
//! semantics because those do not appear in the atomic-entity shape.

use crate::{
    domain::{
        entity::Entity,
        provenance::{atomic_entity::AtomicEntity, error::ProvenanceError, proof::DataIntegrityProof},
    },
    jsonld::JsonLdExpandable,
};
use oxrdf::{BlankNode, Dataset, GraphName, Literal, NamedNode, Quad, Subject, Term};
use rdf_canon::canonicalize;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// SHA-256 output width in bytes.
pub const SHA256_LEN: usize = 32;

/// RDF IRI for `rdf:type`.
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
/// XSD IRI for dateTime literals.
const XSD_DATE_TIME: &str = "http://www.w3.org/2001/XMLSchema#dateTime";
/// Default vocab prefix applied to unresolved NGSI-LD attribute names.
const DEFAULT_VOCAB: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

/// Compute the RDFC-1.0 canonical hash of an unsigned atomic entity.
pub fn hash_unsigned_atomic_entity(entity: &AtomicEntity) -> Result<[u8; SHA256_LEN], ProvenanceError> {
    let dataset = unsigned_atomic_entity_dataset(entity)?;
    hash_dataset(&dataset)
}

/// Compute the RDFC-1.0 canonical hash of the proof configuration
/// (proof minus `proofValue`).
pub fn hash_proof_config(proof: &DataIntegrityProof) -> Result<[u8; SHA256_LEN], ProvenanceError> {
    let dataset = proof_config_dataset(proof)?;
    hash_dataset(&dataset)
}

/// Produce the signing input bytes `hash_proof_config || hash_data`.
pub fn signing_input(entity: &AtomicEntity, proof: &DataIntegrityProof) -> Result<Vec<u8>, ProvenanceError> {
    let hash_proof = hash_proof_config(proof)?;
    let hash_data = hash_unsigned_atomic_entity(entity)?;
    let mut buffer = Vec::with_capacity(SHA256_LEN * 2);
    buffer.extend_from_slice(&hash_proof);
    buffer.extend_from_slice(&hash_data);
    Ok(buffer)
}

fn hash_dataset(dataset: &Dataset) -> Result<[u8; SHA256_LEN], ProvenanceError> {
    let canonical = canonicalize(dataset).map_err(|e| ProvenanceError::CanonicalizationFailed(e.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let digest = hasher.finalize();
    let mut out = [0_u8; SHA256_LEN];
    out.copy_from_slice(&digest);
    Ok(out)
}

fn unsigned_atomic_entity_dataset(entity: &AtomicEntity) -> Result<Dataset, ProvenanceError> {
    let map = entity.to_unsigned_map()?;
    let value = serde_json::to_value(&map).map_err(|e| ProvenanceError::SerializationFailed(e.to_string()))?;
    let parsed: Entity = serde_json::from_value(value).map_err(|e| ProvenanceError::SerializationFailed(e.to_string()))?;
    let expanded = parsed.to_expanded();
    expanded_value_to_dataset(&expanded)
}

fn proof_config_dataset(proof: &DataIntegrityProof) -> Result<Dataset, ProvenanceError> {
    let expanded = expand_proof_config(proof);
    expanded_value_to_dataset(&expanded)
}

fn expand_proof_config(proof: &DataIntegrityProof) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "@type".to_string(),
        Value::Array(vec![Value::String("https://w3id.org/security#DataIntegrityProof".to_string())]),
    );
    obj.insert(
        "https://w3id.org/security#cryptosuite".to_string(),
        serde_json::json!([{
            "@value": proof.cryptosuite.to_string(),
            "@type": "https://w3id.org/security#cryptosuiteString"
        }]),
    );
    obj.insert(
        "https://w3id.org/security#verificationMethod".to_string(),
        serde_json::json!([{ "@id": proof.verification_method.to_string() }]),
    );
    obj.insert(
        "https://w3id.org/security#proofPurpose".to_string(),
        serde_json::json!([{ "@id": format!("https://w3id.org/security#{}", proof.proof_purpose) }]),
    );
    if let Some(ref created) = proof.created {
        obj.insert(
            "http://purl.org/dc/terms/created".to_string(),
            serde_json::json!([{
                "@value": created.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "@type": XSD_DATE_TIME
            }]),
        );
    }
    if let Some(ref expires) = proof.expires {
        obj.insert(
            "https://w3id.org/security#expiration".to_string(),
            serde_json::json!([{
                "@value": expires.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "@type": XSD_DATE_TIME
            }]),
        );
    }
    if let Some(ref challenge) = proof.challenge {
        obj.insert("https://w3id.org/security#challenge".to_string(), serde_json::json!([{ "@value": challenge }]));
    }
    Value::Array(vec![Value::Object(obj)])
}

fn expanded_value_to_dataset(value: &Value) -> Result<Dataset, ProvenanceError> {
    let mut state = WalkState::default();
    match value {
        Value::Array(items) => {
            for item in items {
                walk_node(item, &mut state)?;
            }
        }
        Value::Object(_) => {
            walk_node(value, &mut state)?;
        }
        _other => return Err(ProvenanceError::CanonicalizationFailed("expanded JSON-LD must be an object or array".into())),
    }
    Ok(Dataset::from_iter(state.quads))
}

#[derive(Default)]
struct WalkState {
    quads: Vec<Quad>,
    blank_counter: u32,
}

impl WalkState {
    fn fresh_blank(&mut self) -> BlankNode {
        let id = format!("b{}", self.blank_counter);
        self.blank_counter += 1;
        BlankNode::new(id).unwrap_or_else(|_| BlankNode::default())
    }
}

fn walk_node(value: &Value, state: &mut WalkState) -> Result<Subject, ProvenanceError> {
    let object = value
        .as_object()
        .ok_or_else(|| ProvenanceError::CanonicalizationFailed("expanded JSON-LD node must be an object".into()))?;

    let subject: Subject = match object.get("@id").and_then(Value::as_str) {
        Some(iri) => resolve_named_node(iri)?.into(),
        None => state.fresh_blank().into(),
    };

    if let Some(types) = object.get("@type").and_then(Value::as_array) {
        let rdf_type = NamedNode::new(RDF_TYPE).map_err(|e| ProvenanceError::CanonicalizationFailed(e.to_string()))?;
        for type_value in types {
            let iri = type_value
                .as_str()
                .ok_or_else(|| ProvenanceError::CanonicalizationFailed("@type entry must be a string".into()))?;
            let object_term: Term = resolve_named_node(iri)?.into();
            state
                .quads
                .push(Quad::new(subject.clone(), rdf_type.clone(), object_term, GraphName::DefaultGraph));
        }
    }

    for (key, payload) in object {
        if key == "@id" || key == "@type" || key.starts_with('@') {
            continue;
        }
        let predicate = resolve_named_node(key)?;
        let values = payload
            .as_array()
            .ok_or_else(|| ProvenanceError::CanonicalizationFailed(format!("property {key} must be an array")))?;
        for item in values {
            let term = walk_property_value(item, state)?;
            state.quads.push(Quad::new(subject.clone(), predicate.clone(), term, GraphName::DefaultGraph));
        }
    }

    Ok(subject)
}

fn walk_property_value(value: &Value, state: &mut WalkState) -> Result<Term, ProvenanceError> {
    let object = value
        .as_object()
        .ok_or_else(|| ProvenanceError::CanonicalizationFailed("expanded property value must be an object".into()))?;

    if let Some(raw_value) = object.get("@value") {
        return Ok(build_literal(raw_value, object)?.into());
    }
    if let Some(id) = object.get("@id").and_then(Value::as_str)
        && object.len() == 1
    {
        return Ok(resolve_named_node(id)?.into());
    }
    let nested_subject = walk_node(value, state)?;
    Ok(match nested_subject {
        Subject::NamedNode(n) => Term::NamedNode(n),
        Subject::BlankNode(b) => Term::BlankNode(b),
    })
}

fn resolve_named_node(raw: &str) -> Result<NamedNode, ProvenanceError> {
    let iri = if looks_like_absolute_iri(raw) {
        raw.to_string()
    } else {
        format!("{DEFAULT_VOCAB}{raw}")
    };
    NamedNode::new(iri).map_err(|e| ProvenanceError::CanonicalizationFailed(e.to_string()))
}

fn looks_like_absolute_iri(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let is_alpha = b.is_ascii_alphabetic();
        let is_scheme_char = b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.';
        if i == 0 && !is_alpha {
            return false;
        }
        if b == b':' {
            return i > 0;
        }
        if !is_scheme_char {
            return false;
        }
        i += 1;
    }
    false
}

fn build_literal(raw: &Value, object: &serde_json::Map<String, Value>) -> Result<Literal, ProvenanceError> {
    let lexical = match raw {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null => String::new(),
        _other => serde_json::to_string(raw).map_err(|e| ProvenanceError::SerializationFailed(e.to_string()))?,
    };
    if let Some(datatype) = object.get("@type").and_then(Value::as_str) {
        let dt = resolve_named_node(datatype)?;
        return Ok(Literal::new_typed_literal(lexical, dt));
    }
    if let Some(lang) = object.get("@language").and_then(Value::as_str) {
        return Literal::new_language_tagged_literal(lexical, lang).map_err(|e| ProvenanceError::CanonicalizationFailed(e.to_string()));
    }
    Ok(Literal::new_simple_literal(lexical))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    fn sample_atomic_entity() -> AtomicEntity {
        let sig64 = multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]);
        serde_json::from_value(serde_json::json!({
            "id": "urn:ngsi-ld:Store:002",
            "type": "Store",
            "address": { "type": "Property", "value": 1 },
            "ngsildproof": {
                "type": "Property",
                "value": {
                    "type": "DataIntegrityProof",
                    "cryptosuite": "eddsa-rdfc-2022",
                    "verificationMethod": "https://example.com/keys/1",
                    "proofPurpose": "assertionMethod",
                    "proofValue": sig64
                }
            }
        }))
        .unwrap()
    }

    #[test]
    fn rdfc_hashes_unsigned_atomic_entity_deterministically() {
        let ae = sample_atomic_entity();
        let a = hash_unsigned_atomic_entity(&ae).expect("hash");
        let b = hash_unsigned_atomic_entity(&ae).expect("hash");
        pretty_assertions::assert_eq!(a, b);
    }

    #[test]
    fn rdfc_hashes_proof_config_deterministically() {
        let ae = sample_atomic_entity();
        let a = hash_proof_config(&ae.proof.value).expect("hash");
        let b = hash_proof_config(&ae.proof.value).expect("hash");
        pretty_assertions::assert_eq!(a, b);
    }

    #[test]
    fn rdfc_canonicalization_is_deterministic_under_key_reordering() {
        // Construct two entities whose JSON key order differs. RDFC must
        // produce the same canonical dataset since RDF has no notion of
        // property order. Directly exercises INT-2.
        let sig64 = multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]);
        let ae_a: AtomicEntity = serde_json::from_value(serde_json::json!({
            "id": "urn:ngsi-ld:Store:002",
            "type": "Store",
            "address": { "type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00Z" },
            "ngsildproof": {
                "type": "Property",
                "value": {
                    "type": "DataIntegrityProof",
                    "cryptosuite": "eddsa-rdfc-2022",
                    "verificationMethod": "https://example.com/keys/1",
                    "proofPurpose": "assertionMethod",
                    "proofValue": sig64
                }
            }
        }))
        .unwrap();
        let ae_b: AtomicEntity = serde_json::from_value(serde_json::json!({
            "type": "Store",
            "id": "urn:ngsi-ld:Store:002",
            "ngsildproof": {
                "type": "Property",
                "value": {
                    "proofPurpose": "assertionMethod",
                    "proofValue": sig64,
                    "type": "DataIntegrityProof",
                    "verificationMethod": "https://example.com/keys/1",
                    "cryptosuite": "eddsa-rdfc-2022"
                }
            },
            "address": { "observedAt": "2024-01-01T00:00:00Z", "type": "Property", "value": 1 }
        }))
        .unwrap();
        pretty_assertions::assert_eq!(hash_unsigned_atomic_entity(&ae_a).unwrap(), hash_unsigned_atomic_entity(&ae_b).unwrap());
        pretty_assertions::assert_eq!(hash_proof_config(&ae_a.proof.value).unwrap(), hash_proof_config(&ae_b.proof.value).unwrap());
    }

    #[test]
    fn rdfc_signing_input_length_is_two_sha256() {
        let ae = sample_atomic_entity();
        let bytes = signing_input(&ae, &ae.proof.value).expect("signing input");
        pretty_assertions::assert_eq!(bytes.len(), SHA256_LEN * 2);
    }

    #[test]
    fn rdfc_detects_core_tamper() {
        let sig64 = multibase::encode(multibase::Base::Base58Btc, vec![0_u8; 64]);
        let ae_a: AtomicEntity = serde_json::from_value(serde_json::json!({
            "id": "urn:ngsi-ld:Store:002",
            "type": "Store",
            "address": { "type": "Property", "value": 1 },
            "ngsildproof": {
                "type": "Property",
                "value": {
                    "type": "DataIntegrityProof",
                    "cryptosuite": "eddsa-rdfc-2022",
                    "verificationMethod": "https://example.com/keys/1",
                    "proofPurpose": "assertionMethod",
                    "proofValue": sig64
                }
            }
        }))
        .unwrap();
        let ae_b: AtomicEntity = serde_json::from_value(serde_json::json!({
            "id": "urn:ngsi-ld:Store:002",
            "type": "Store",
            "address": { "type": "Property", "value": 2 },
            "ngsildproof": {
                "type": "Property",
                "value": {
                    "type": "DataIntegrityProof",
                    "cryptosuite": "eddsa-rdfc-2022",
                    "verificationMethod": "https://example.com/keys/1",
                    "proofPurpose": "assertionMethod",
                    "proofValue": sig64
                }
            }
        }))
        .unwrap();
        assert_ne!(hash_unsigned_atomic_entity(&ae_a).unwrap(), hash_unsigned_atomic_entity(&ae_b).unwrap());
    }
}
