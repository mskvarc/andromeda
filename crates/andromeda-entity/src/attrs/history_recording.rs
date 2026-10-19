//! Every attribute-level write records, for the Temporal Evolution, the
//! instances it left in the Entity rather than the Fragment that asked for them
//! (clauses 4.5.7 and 4.5.8), proven without a live broker.
//!
//! The fixture Building holds `category` = "office" with an `accuracy`
//! sub-Property, so a write that recorded its Fragment would lose that
//! sub-Property from history even where the stored Entity kept it.

use crate::{
    admission_fixture::{admitted_attribute, admitted_written},
    history_recording_assertions::{assert_observes_stored_instances, observed_attributes, observed_default_instance, recorded_observation},
    notification_recording_harness::{
        ACCURACY_IRI,
        CATEGORY_IRI,
        NAME_IRI,
        attribute_name,
        core_context,
        entity_uri,
        live_target,
        recording_service,
        single_recorded_write,
    },
    write_kind::{AppendAttributes, PartialUpdateAttribute, ReplaceAttribute, UpdateAttributes},
    write_target::WriteTarget,
};
use andromeda_context_commit::history_record::HistoryRecord;
use andromeda_ngsi_ld_entity::operation::selection::OverwritePolicy;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use pretty_assertions::assert_eq;
use serde_json::json;

/// Clause 5.5.8 EXAMPLE 1: a partial update leaves the sub-attributes the
/// Fragment omits in place, and the recorded instance is the patched one.
#[tokio::test]
async fn partial_update_attr_records_the_patched_instance_keeping_its_untouched_sub_property() {
    let (service, recorded) = recording_service().await;
    let attribute = attribute_name("category");
    let admitted = admitted_attribute::<PartialUpdateAttribute>(&service, &attribute, json!({"type": "Property", "value": "retail"}), &core_context()).await;
    service
        .partial_update_attr(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the partial update lands");

    let write = single_recorded_write(&recorded);
    assert_observes_stored_instances(&write, &[CATEGORY_IRI]);
    let observation = recorded_observation(&write).expect("an observation");
    let category = observed_default_instance(&observation, CATEGORY_IRI);
    assert_eq!(category[properties::VALUE.as_str()][0]["@value"], json!("retail"));
    assert_eq!(
        category[ACCURACY_IRI][0][properties::VALUE.as_str()][0]["@value"],
        json!("high"),
        "the recorded instance keeps the sub-Property the Fragment left out"
    );
}

/// A sub-Property of the Attribute Fragment belongs to the instance; it is
/// never recorded as an attribute of the Entity in its own right.
#[tokio::test]
async fn partial_update_attr_does_not_record_a_fragment_sub_property_as_an_entity_attribute() {
    let (service, recorded) = recording_service().await;
    let attribute = attribute_name("category");
    let admitted = admitted_attribute::<PartialUpdateAttribute>(
        &service,
        &attribute,
        json!({"type": "Property", "value": "retail", "name": {"type": "Property", "value": "annotated"}}),
        &core_context(),
    )
    .await;
    service
        .partial_update_attr(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the partial update lands");

    let write = single_recorded_write(&recorded);
    let observation = recorded_observation(&write).expect("an observation");
    assert_eq!(observed_attributes(&observation).into_iter().collect::<Vec<_>>(), vec![CATEGORY_IRI.to_owned()]);
    assert!(
        observed_default_instance(&observation, CATEGORY_IRI).get(NAME_IRI).is_some(),
        "the sub-Property is recorded inside the instance it belongs to"
    );
}

/// Clause 5.5.8 hands an NGSI-LD Null Fragment to clause 5.6.5: the instance
/// ends, which history records as a tombstone and never as an observed value.
#[tokio::test]
async fn partial_update_attr_to_the_null_sentinel_records_only_the_tombstone() {
    let (service, recorded) = recording_service().await;
    let attribute = attribute_name("category");
    let admitted =
        admitted_attribute::<PartialUpdateAttribute>(&service, &attribute, json!({"type": "Property", "value": "urn:ngsi-ld:null"}), &core_context()).await;
    service
        .partial_update_attr(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the deleting partial update lands");

    let write = single_recorded_write(&recorded);
    assert!(recorded_observation(&write).is_none(), "a deletion observes no value");
    let [HistoryRecord::Tombstone(removed)] = write.history.as_slice() else {
        panic!("the deletion records exactly one tombstone: {:?}", write.history);
    };
    assert_eq!(removed.key.attr, attribute_name(CATEGORY_IRI));
    assert_eq!(removed.key.dataset_id, None);
}

/// Clause 5.6.19.4 replaces the instance whole: the recorded instance is the
/// replacement as stored, without the sub-Property it dropped.
#[tokio::test]
async fn replace_attr_records_the_stored_replacement() {
    let (service, recorded) = recording_service().await;
    let attribute = attribute_name("category");
    let admitted = admitted_attribute::<ReplaceAttribute>(&service, &attribute, json!({"type": "Property", "value": "retail"}), &core_context()).await;
    service
        .replace_attr(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the replace lands");

    let write = single_recorded_write(&recorded);
    assert_observes_stored_instances(&write, &[CATEGORY_IRI]);
    let observation = recorded_observation(&write).expect("an observation");
    let category = observed_default_instance(&observation, CATEGORY_IRI);
    assert_eq!(category[properties::VALUE.as_str()][0]["@value"], json!("retail"));
    assert!(category.get(ACCURACY_IRI).is_none(), "the replacement drops the sub-Property");
    assert!(
        category.get(properties::CREATED_AT.as_str()).is_some(),
        "the recorded instance carries the system times the stored one does"
    );
}

/// Clause 5.5.8 EXAMPLE 2: Update Attributes overwrites the whole instance.
#[tokio::test]
async fn update_attrs_records_the_whole_instance_it_wrote() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_written::<UpdateAttributes>(&service, json!({"category": {"type": "Property", "value": "retail"}}), &core_context()).await;
    service
        .update_attrs(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the update lands");

    let write = single_recorded_write(&recorded);
    assert_observes_stored_instances(&write, &[CATEGORY_IRI]);
    let observation = recorded_observation(&write).expect("an observation");
    let category = observed_default_instance(&observation, CATEGORY_IRI);
    assert!(category.get(ACCURACY_IRI).is_none(), "the whole-instance overwrite drops the sub-Property");
}

/// Clause 5.6.2.4 ignores a `scope` the target Entity does not hold, so the
/// write records no Scope.
#[tokio::test]
async fn update_attrs_records_no_scope_it_ignored() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_written::<UpdateAttributes>(
        &service,
        json!({"scope": "/Madrid", "category": {"type": "Property", "value": "retail"}}),
        &core_context(),
    )
    .await;
    service
        .update_attrs(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the update lands");

    let write = single_recorded_write(&recorded);
    let observation = recorded_observation(&write).expect("an observation");
    assert!(
        observation[0].get(properties::SCOPE.as_str()).is_none(),
        "an ignored Scope is not recorded: {observation}"
    );
    assert_observes_stored_instances(&write, &[CATEGORY_IRI]);
}

/// Clause 5.6.3.4: with overwriting disabled an attribute the Entity already
/// holds is left untouched, so it is not recorded; an appended one is.
#[tokio::test]
async fn append_attrs_without_overwrite_records_only_the_attributes_it_appended() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_written::<AppendAttributes>(
        &service,
        json!({
            "category": {"type": "Property", "value": "retail"},
            "name": {"type": "Property", "value": "lobby"}
        }),
        &core_context(),
    )
    .await;
    service
        .append_attrs(&live_target(), WriteTarget::new(&entity_uri(), None), admitted, OverwritePolicy::Preserve)
        .await
        .expect("the append lands");

    let write = single_recorded_write(&recorded);
    assert_observes_stored_instances(&write, &[NAME_IRI]);
}

/// A no-overwrite append that appended nothing records no observation at all.
/// It changed nothing, so it stores no write either (clause 4.8 keeps the
/// entity's `modifiedAt`).
#[tokio::test]
async fn append_attrs_without_overwrite_that_appended_nothing_records_no_observation() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_written::<AppendAttributes>(&service, json!({"category": {"type": "Property", "value": "retail"}}), &core_context()).await;
    service
        .append_attrs(&live_target(), WriteTarget::new(&entity_uri(), None), admitted, OverwritePolicy::Preserve)
        .await
        .expect("the append lands");

    let writes = recorded.lock().expect("the recording buffer");
    assert!(
        writes.iter().all(|write| recorded_observation(write).is_none()),
        "the skipped value is not recorded"
    );
    assert!(writes.is_empty(), "an append that changed nothing stores no write");
}

/// Clause 5.6.3.4: an append that overwrites records the stored instances.
#[tokio::test]
async fn append_attrs_with_overwrite_records_the_stored_instances() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_written::<AppendAttributes>(
        &service,
        json!({
            "category": {"type": "Property", "value": "retail"},
            "name": {"type": "Property", "value": "lobby"}
        }),
        &core_context(),
    )
    .await;
    service
        .append_attrs(&live_target(), WriteTarget::new(&entity_uri(), None), admitted, OverwritePolicy::Overwrite)
        .await
        .expect("the append lands");

    let write = single_recorded_write(&recorded);
    assert_observes_stored_instances(&write, &[CATEGORY_IRI, NAME_IRI]);
}

/// An empty Attribute Fragment is an RFC 7396 patch naming nothing (clause
/// 5.4): the partial update lands and leaves the instance as it was.
#[tokio::test]
async fn partial_update_attr_with_an_empty_fragment_leaves_the_instance_as_it_was() {
    let (service, recorded) = recording_service().await;
    let attribute = attribute_name("category");
    let admitted = admitted_attribute::<PartialUpdateAttribute>(&service, &attribute, json!({}), &core_context()).await;
    service
        .partial_update_attr(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the empty fragment is admitted");

    let writes = recorded.lock().expect("the recording buffer");
    for write in writes.iter() {
        let stored = serde_json::to_value(write.entity.record().expect("an entity record").state.as_ref()).expect("the state encodes");
        let category = &stored[0][CATEGORY_IRI][0];
        assert_eq!(category[properties::VALUE.as_str()][0]["@value"], json!("office"), "{stored}");
        assert_eq!(category[ACCURACY_IRI][0][properties::VALUE.as_str()][0]["@value"], json!("high"), "{stored}");
    }
}

/// Clauses 5.4, 5.5.8 and 5.6.4: an empty Attribute Fragment changes nothing,
/// so nothing is stored and the entity keeps its `modifiedAt` (clause 4.8).
#[tokio::test]
async fn partial_update_attr_with_an_empty_fragment_stores_nothing() {
    let (service, recorded) = recording_service().await;
    let attribute = attribute_name("category");
    let admitted = admitted_attribute::<PartialUpdateAttribute>(&service, &attribute, json!({}), &core_context()).await;
    service
        .partial_update_attr(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the empty fragment is admitted");

    assert!(recorded.lock().expect("the recording buffer").is_empty(), "an empty fragment stores no write");
}

/// Clauses 5.4, 5.5.8 and 5.6.2: an empty Entity Fragment stores nothing on
/// Update Attributes and reports no attribute as updated.
#[tokio::test]
async fn update_attrs_with_an_empty_fragment_stores_nothing() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_written::<UpdateAttributes>(&service, json!({}), &core_context()).await;
    let (requested, not_updated) = service
        .update_attrs(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the empty fragment is admitted");

    assert!(requested.is_empty() && not_updated.is_empty(), "{requested:?} {not_updated:?}");
    assert!(recorded.lock().expect("the recording buffer").is_empty(), "an empty fragment stores no write");
}

/// Clauses 5.4 and 5.6.3: an empty Entity Fragment stores nothing on Append
/// Attributes, with or without overwriting.
#[tokio::test]
async fn append_attrs_with_an_empty_fragment_stores_nothing() {
    for overwrite in [OverwritePolicy::Overwrite, OverwritePolicy::Preserve] {
        let (service, recorded) = recording_service().await;
        let admitted = admitted_written::<AppendAttributes>(&service, json!({}), &core_context()).await;
        service
            .append_attrs(&live_target(), WriteTarget::new(&entity_uri(), None), admitted, overwrite)
            .await
            .expect("the empty fragment is admitted");

        assert!(
            recorded.lock().expect("the recording buffer").is_empty(),
            "{overwrite:?}: an empty fragment stores no write"
        );
    }
}

/// The fixture `category` restated whole: its value and its `accuracy`
/// sub-Property as the Building holds them.
fn restated_category() -> serde_json::Value {
    json!({"type": "Property", "value": "office", "accuracy": {"type": "Property", "value": "high"}})
}

/// Clauses 4.3.1, 4.8 and 5.8.6: Update Attributes restating the held
/// instance modifies nothing, so it stores no write.
#[tokio::test]
async fn update_attrs_restating_the_held_instance_stores_nothing() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_written::<UpdateAttributes>(&service, json!({"category": restated_category()}), &core_context()).await;
    service
        .update_attrs(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the update lands");

    assert!(recorded.lock().expect("the recording buffer").is_empty(), "a restatement stores no write");
}

/// Clause 5.6.3.4 with overwriting: appending the held instance restated
/// modifies nothing, so it stores no write.
#[tokio::test]
async fn append_attrs_with_overwrite_restating_the_held_instance_stores_nothing() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_written::<AppendAttributes>(&service, json!({"category": restated_category()}), &core_context()).await;
    service
        .append_attrs(&live_target(), WriteTarget::new(&entity_uri(), None), admitted, OverwritePolicy::Overwrite)
        .await
        .expect("the append lands");

    assert!(recorded.lock().expect("the recording buffer").is_empty(), "a restatement stores no write");
}

/// Clause 5.5.8: a partial update restating the held value leaves the
/// instance as it was, so it stores no write.
#[tokio::test]
async fn partial_update_attr_restating_the_held_value_stores_nothing() {
    let (service, recorded) = recording_service().await;
    let attribute = attribute_name("category");
    let admitted = admitted_attribute::<PartialUpdateAttribute>(&service, &attribute, json!({"value": "office"}), &core_context()).await;
    service
        .partial_update_attr(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the partial update lands");

    assert!(recorded.lock().expect("the recording buffer").is_empty(), "a restatement stores no write");
}

/// `observedAt` is a member of the instance (clause 4.5.1): the held value
/// observed at an instant it did not carry is a modification and is recorded.
#[tokio::test]
async fn partial_update_attr_restating_the_held_value_at_a_new_observed_at_records_it() {
    let (service, recorded) = recording_service().await;
    let attribute = attribute_name("category");
    let admitted = admitted_attribute::<PartialUpdateAttribute>(
        &service,
        &attribute,
        json!({"value": "office", "observedAt": "2026-10-19T10:00:00Z"}),
        &core_context(),
    )
    .await;
    service
        .partial_update_attr(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the partial update lands");

    let write = single_recorded_write(&recorded);
    assert_observes_stored_instances(&write, &[CATEGORY_IRI]);
}

/// Clause 5.6.19.4: replacing the held instance with itself modifies nothing,
/// so it stores no write.
#[tokio::test]
async fn replace_attr_restating_the_held_instance_stores_nothing() {
    let (service, recorded) = recording_service().await;
    let attribute = attribute_name("category");
    let admitted = admitted_attribute::<ReplaceAttribute>(&service, &attribute, restated_category(), &core_context()).await;
    service
        .replace_attr(&live_target(), WriteTarget::new(&entity_uri(), None), admitted)
        .await
        .expect("the replace lands");

    assert!(recorded.lock().expect("the recording buffer").is_empty(), "a restatement stores no write");
}
