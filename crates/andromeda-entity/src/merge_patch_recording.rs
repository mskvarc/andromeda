//! Merge Entity applies the typed patch a merge fragment is read into
//! (clause 5.5.12): an instance may leave its value out, a member written as
//! the NGSI-LD Null removes what it stands on, and a whole member written as
//! the Null removes the attribute, proven on the Entity the merge stores.

use crate::{
    admission_fixture::{admitted_context, admitted_entity},
    merge_options::MergeOptions,
    notification_recording_harness::{ACCURACY_IRI, CATEGORY_IRI, core_context, entity_uri, live_target, recording_service, single_recorded_write},
    write_kind::MergeEntity,
    write_target::WriteTarget,
};
use andromeda_ngsi_ld_model::{attribute::merge_member::MergeMember, entity::EntityPatch};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// The expanded `hasValue` member.
const HAS_VALUE: &str = "https://uri.etsi.org/ngsi-ld/hasValue";

/// The expanded `unitCode` member.
const UNIT_CODE: &str = "https://uri.etsi.org/ngsi-ld/unitCode";

/// The expanded `observedAt` member.
const OBSERVED_AT: &str = "https://uri.etsi.org/ngsi-ld/observedAt";

/// Merge `fragment` into the stored Building, returning the Entity node the
/// merge stores.
async fn merged(fragment: Value) -> Value {
    let (service, recorded) = recording_service().await;
    let fragment: EntityPatch<MergeMember> = serde_json::from_value(fragment).expect("a merge fragment");
    let admitted = admitted_entity::<MergeEntity>(&service, &fragment, &core_context()).await;
    service
        .merge(
            &live_target(),
            WriteTarget::new(&entity_uri(), None),
            &fragment,
            admitted,
            &admitted_context(&service, &core_context()).await,
            MergeOptions::default(),
        )
        .await
        .expect("the merge lands");
    let write = single_recorded_write(&recorded);
    let stored = serde_json::to_value(write.entity.record().expect("the merge stores the entity").state.as_ref()).expect("the state encodes");
    match stored {
        Value::Array(mut nodes) if !nodes.is_empty() => nodes.swap_remove(0),
        other @ (Value::Array(_) | Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => other,
    }
}

/// The stored default instance of `category`.
fn category(node: &Value) -> &Value {
    &node[CATEGORY_IRI][0]
}

/// An instance that leaves its value out merges the members it writes
/// and keeps the value held ("up to an arbitrary depth", clause 5.5.12).
#[tokio::test]
async fn a_merge_instance_without_its_value_member_merges_its_other_members() {
    let node = merged(json!({"category": {"type": "Property", "unitCode": "FAH"}})).await;
    assert_eq!(category(&node)[HAS_VALUE][0]["@value"], json!("office"), "{node}");
    assert_eq!(category(&node)[UNIT_CODE][0]["@value"], json!("FAH"), "{node}");
    assert_eq!(category(&node)[ACCURACY_IRI][0][HAS_VALUE][0]["@value"], json!("high"), "{node}");
}

/// The NGSI-LD Null on an instance member and on a sub-attribute
/// removes each (clause 5.5.12), the value the instance writes landing.
#[tokio::test]
async fn a_null_on_an_instance_member_removes_it_in_a_merge() {
    let node = merged(json!({
        "category": {"type": "Property", "value": "retail", "observedAt": "urn:ngsi-ld:null", "unitCode": "urn:ngsi-ld:null", "accuracy": "urn:ngsi-ld:null"}
    }))
    .await;
    assert_eq!(category(&node)[HAS_VALUE][0]["@value"], json!("retail"), "{node}");
    for removed in [ACCURACY_IRI, UNIT_CODE, OBSERVED_AT] {
        assert!(category(&node).get(removed).is_none(), "{removed} is removed: {node}");
    }
    assert!(!node.to_string().contains("urn:ngsi-ld:null"), "no sentinel is stored: {node}");
}

/// Clauses 5.4 and 5.5.12: a merge fragment that names no member is a JSON
/// Merge Patch that returns its target unchanged, so nothing is stored.
#[tokio::test]
async fn a_merge_with_an_empty_fragment_stores_nothing() {
    let (service, recorded) = recording_service().await;
    let fragment: EntityPatch<MergeMember> = serde_json::from_value(json!({})).expect("a merge fragment");
    let admitted = admitted_entity::<MergeEntity>(&service, &fragment, &core_context()).await;
    service
        .merge(
            &live_target(),
            WriteTarget::new(&entity_uri(), None),
            &fragment,
            admitted,
            &admitted_context(&service, &core_context()).await,
            MergeOptions::default(),
        )
        .await
        .expect("the empty fragment is admitted");

    assert!(recorded.lock().expect("the recording buffer").is_empty(), "an empty fragment stores no write");
}

/// The NGSI-LD Null in place of a whole member removes the attribute
/// (clause 5.5.12).
#[tokio::test]
async fn a_bare_null_member_reads_as_a_member_deletion_in_a_merge() {
    let node = merged(json!({"category": "urn:ngsi-ld:null"})).await;
    assert!(node.get(CATEGORY_IRI).is_none(), "{node}");
}
