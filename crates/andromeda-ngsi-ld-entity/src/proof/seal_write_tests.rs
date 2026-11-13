//! A written `ngsildproof` turned into state and applied to a held one
//! (clause 4.5.2.2, ETSI TS 104 179 V1.1.1 clauses 5.1.2 and 5.2.3).

use crate::{
    admission::{null_sentinel_admission::NullSentinelAdmission, written_families::WrittenFamilies},
    document::{EntityDocument, instance_document::InstanceDocument},
    entity_state::EntityState,
    family_value::FamilyValue,
    instance::Instance,
    member_role::ProofSeal,
    operation::{
        instance_patch::{InstancePatchDepth, patch_instance},
        mutation_error::MutationError,
    },
    proof::{attribute_proof::AttributeProof, proof_fault::ProofFault, proof_member::ProofMember},
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use serde_json::{Value, json};

const NGSI_LD: &str = "https://uri.etsi.org/ngsi-ld/";
const SECURITY: &str = "https://w3id.org/security#";
const ADDRESS: &str = "https://example.org/address";

fn ngsi(term: &str) -> String {
    format!("{NGSI_LD}{term}")
}

fn security(term: &str) -> String {
    format!("{SECURITY}{term}")
}

fn name(raw: &str) -> AttributeName {
    raw.parse().expect("an attribute name")
}

/// The annex C.11 proof, expanded by the W3C Data Integrity v2 context.
fn proof() -> Value {
    json!({
        "@type": [security("DataIntegrityProof")],
        "http://purl.org/dc/terms/created": [{"@type": "http://www.w3.org/2001/XMLSchema#dateTime", "@value": "2025-01-27T21:02:24Z"}],
        security("verificationMethod"): [{"@id": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT"}],
        security("cryptosuite"): [{"@type": security("cryptosuiteString"), "@value": "eddsa-rdfc-2022"}],
        security("proofPurpose"): [{"@id": security("assertionMethod")}],
        security("proofValue"): [{"@type": security("multibase"), "@value": "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS"}]
    })
}

/// An expanded `ngsildproof` instance of `family` holding `value`, carrying
/// `seals`.
fn seal_of(family: &str, value: &Value, seals: &Value) -> Value {
    let mut instance = json!({"@type": [ngsi(family)], ngsi("hasValue"): [value]});
    if let (Some(instance), Some(seals)) = (instance.as_object_mut(), seals.as_object()) {
        instance.extend(seals.iter().map(|(key, value)| (key.clone(), value.clone())));
    }
    instance
}

/// Both seals, naming `urn:ngsi-ld:Store:002` of type `Store`.
fn both_seals() -> Value {
    json!({
        ngsi("entityIdSealed"): [{"@value": "urn:ngsi-ld:Store:002"}],
        ngsi("entityTypeSealed"): [{"@id": "https://example.org/Store"}]
    })
}

/// The state of a Store whose `address` carries `seal` as its `ngsildproof`.
fn sealed_store(seal: &Value) -> Result<EntityState, EntityRefusal> {
    let document = json!([{
        "@id": "urn:ngsi-ld:Store:002",
        "@type": ["https://example.org/Store"],
        ADDRESS: [{
            "@type": [ngsi("Property")],
            ngsi("hasValue"): [{"@value": "Tiger Street 4"}],
            ngsi("ngsildproof"): [seal]
        }]
    }]);
    EntityState::try_from(EntityDocument::from_slice(document.to_string().as_bytes())?)
}

/// The seal the `address` of `state` carries.
fn held_seal(state: &EntityState) -> &Instance {
    state
        .attributes()
        .get(ADDRESS)
        .and_then(|instances| instances.get(None))
        .and_then(|address| address.sub_attributes().get(&ngsi("ngsildproof")))
        .and_then(|instances| instances.get(None))
        .expect("the seal")
}

/// The fault `refusal` reports, which is a seal's.
fn fault(refusal: &EntityRefusal) -> ProofFault {
    let EntityRefusal::Proof { fault, .. } = refusal else {
        panic!("a seal refusal expected, got {refusal:?}");
    };
    *fault
}

fn typed(instance: &Instance) -> &AttributeProof {
    instance.value().proof().expect("a typed seal")
}

#[test]
fn a_written_seal_is_held_typed() {
    let state = sealed_store(&seal_of("Property", &proof(), &both_seals())).expect("a sealed Store");
    let seal = typed(held_seal(&state));
    pretty_assertions::assert_eq!(seal.seals().entity_id().as_str(), "urn:ngsi-ld:Store:002");
    pretty_assertions::assert_eq!(seal.seals().entity_types().as_slice()[0].as_str(), "https://example.org/Store");
    pretty_assertions::assert_eq!(seal.proof().cryptosuite().as_str(), "eddsa-rdfc-2022");
    pretty_assertions::assert_eq!(held_seal(&state).family(), AttributeType::Property);
}

/// Clause 4.5.2.2 makes the value "an object containing the W3C Data integrity
/// proof structure".
#[test]
fn a_seal_whose_value_is_no_object_is_refused() {
    let refusal = sealed_store(&seal_of("Property", &json!({"@value": "a proof"}), &both_seals())).expect_err("no object");
    pretty_assertions::assert_eq!(fault(&refusal), ProofFault::NotOneObject);
}

/// ETSI TS 104 179 clause 5.1.2: a proof contains at least six members.
#[test]
fn a_seal_missing_a_proof_member_is_refused() {
    let mut partial = proof();
    partial.as_object_mut().expect("a node").remove(&security("proofValue"));
    let refusal = sealed_store(&seal_of("Property", &partial, &both_seals())).expect_err("no proofValue");
    pretty_assertions::assert_eq!(fault(&refusal), ProofFault::MissingMember(ProofMember::ProofValue));
}

/// ETSI TS 104 179 clause 5.2.3: the seal holds both the identifier and the
/// type of the entity its proof signs.
#[test]
fn a_seal_missing_a_seal_member_is_refused() {
    let only_id = json!({ngsi("entityIdSealed"): [{"@value": "urn:ngsi-ld:Store:002"}]});
    let refusal = sealed_store(&seal_of("Property", &proof(), &only_id)).expect_err("no entityTypeSealed");
    pretty_assertions::assert_eq!(fault(&refusal), ProofFault::MissingSeal(ProofSeal::EntityType));
}

/// Clause 4.5.2.2 defines `ngsildproof` as "a Property".
#[test]
fn an_ngsildproof_of_another_family_is_refused() {
    let relationship = json!({"@type": [ngsi("Relationship")], ngsi("hasObject"): [{"@id": "urn:ngsi-ld:Proof:1"}]});
    let refusal = sealed_store(&relationship).expect_err("no Property");
    pretty_assertions::assert_eq!(fault(&refusal), ProofFault::NotAProperty(AttributeType::Relationship));
}

/// A seal signs the attribute it is a sub-attribute of; an entity carries no
/// attribute it could sign.
#[test]
fn a_seal_on_the_entity_is_refused() {
    let document = json!([{
        "@id": "urn:ngsi-ld:Store:002",
        "@type": ["https://example.org/Store"],
        ngsi("ngsildproof"): [seal_of("Property", &proof(), &both_seals())]
    }]);
    let written = EntityDocument::from_slice(document.to_string().as_bytes()).expect("an expanded document");
    assert!(matches!(
        written.admit(NullSentinelAdmission::Refused, WrittenFamilies::Final),
        Err(EntityRefusal::ProofOnEntity)
    ));
    assert!(matches!(EntityState::try_from(written), Err(EntityRefusal::ProofOnEntity)));
}

/// A merge (clause 5.5.12) of the proof's `proofValue` alone keeps every
/// other member of the proof and both seals.
#[test]
fn a_merged_proof_value_keeps_the_rest_of_the_seal() {
    let state = sealed_store(&seal_of("Property", &proof(), &both_seals())).expect("a sealed Store");
    let mut held = held_seal(&state).clone();
    let fragment = json!({ngsi("hasValue"): [{security("proofValue"): [{"@type": security("multibase"), "@value": "zNew"}]}]});
    let written = InstanceDocument::from_slice(&name(&ngsi("ngsildproof")), fragment.to_string().as_bytes()).expect("a fragment");
    patch_instance(&name(&ngsi("ngsildproof")), &mut held, &written, InstancePatchDepth::DeepMerge, None).expect("the merge lands");
    let seal = typed(&held);
    pretty_assertions::assert_eq!(seal.proof().proof_value().as_str(), "zNew");
    pretty_assertions::assert_eq!(seal.proof().cryptosuite().as_str(), "eddsa-rdfc-2022");
    pretty_assertions::assert_eq!(seal.seals().entity_id().as_str(), "urn:ngsi-ld:Store:002");
}

/// A partial update (clause 5.5.8) replaces the value whole, so a value that
/// is no complete proof leaves no seal.
#[test]
fn a_partial_update_to_an_incomplete_proof_is_refused() {
    let state = sealed_store(&seal_of("Property", &proof(), &both_seals())).expect("a sealed Store");
    let mut held = held_seal(&state).clone();
    let fragment = json!({ngsi("hasValue"): [{security("proofValue"): [{"@type": security("multibase"), "@value": "zNew"}]}]});
    let written = InstanceDocument::from_slice(&name(&ngsi("ngsildproof")), fragment.to_string().as_bytes()).expect("a fragment");
    let refused = patch_instance(&name(&ngsi("ngsildproof")), &mut held, &written, InstancePatchDepth::PartialUpdate, None);
    let Err(MutationError::Refused(refusal)) = refused else {
        panic!("a refused partial update expected, got {refused:?}");
    };
    pretty_assertions::assert_eq!(fault(&refusal), ProofFault::NotDataIntegrityProof);
    pretty_assertions::assert_eq!(&held, held_seal(&state));
}

/// A written seal replaces its own and keeps the other (clause 5.5.12).
#[test]
fn a_written_seal_replaces_the_held_one() {
    let state = sealed_store(&seal_of("Property", &proof(), &both_seals())).expect("a sealed Store");
    let mut held = held_seal(&state).clone();
    let fragment = json!({ngsi("entityIdSealed"): [{"@value": "urn:ngsi-ld:Store:003"}]});
    let written = InstanceDocument::from_slice(&name(&ngsi("ngsildproof")), fragment.to_string().as_bytes()).expect("a fragment");
    patch_instance(&name(&ngsi("ngsildproof")), &mut held, &written, InstancePatchDepth::DeepMerge, None).expect("the merge lands");
    let seal = typed(&held);
    pretty_assertions::assert_eq!(seal.seals().entity_id().as_str(), "urn:ngsi-ld:Store:003");
    pretty_assertions::assert_eq!(seal.seals().entity_types().as_slice()[0].as_str(), "https://example.org/Store");
    assert!(matches!(held.value(), FamilyValue::Proof(_)));
}
