//! Every batch write records, for the Temporal Evolution and the current-value
//! projection derived from it, the instances it left in each Entity rather than
//! the Fragment that asked for them, exactly as its single-entity sibling does
//! (clauses 5.6.8, 5.6.9 and 5.6.20 run those behaviours per Entity).
//!
//! The fixture Building holds `category` = "office" with an `accuracy`
//! sub-Property, which a Fragment-shaped record would lose.

use crate::{
    admission_fixture::admitted_batch,
    batch::UpsertPolicy,
    history_recording_assertions::{assert_observes_stored_instances, observed_default_instance, recorded_observation},
    notification_recording_harness::{ACCURACY_IRI, CATEGORY_IRI, ENTITY_ID, NAME_IRI, live_target, recording_service, single_recorded_write},
    side_effect::SideEffectPlacement,
    write_kind::{BatchMerge, BatchUpdate, BatchUpsert},
};
use andromeda_context_commit::history_record::HistoryRecord;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::operation::selection::OverwritePolicy;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// A single-entity batch input built from compact attribute members.
fn building(attributes: &Value) -> Entity {
    let mut entity = json!({"id": ENTITY_ID, "type": "Building"});
    if let (Some(entity), Some(attributes)) = (entity.as_object_mut(), attributes.as_object()) {
        entity.extend(attributes.clone());
    }
    serde_json::from_value(entity).expect("the fixture entity is valid")
}

/// Clause 5.6.20 runs the merge of clause 5.6.17: the recorded instance keeps
/// the sub-Property the Fragment omits.
#[tokio::test]
async fn batch_merge_records_the_merged_instance_keeping_its_untouched_sub_property() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_batch(
        &service,
        BatchMerge,
        vec![building(&json!({"category": {"type": "Property", "value": "retail"}}))],
        &ContextSpec::core_only(),
    )
    .await;
    service
        .batch_merge(SideEffectPlacement::Inline(&live_target()), admitted)
        .await
        .expect("the batch merge lands");

    let write = single_recorded_write(&recorded);
    assert_observes_stored_instances(&write, &[CATEGORY_IRI]);
    let observation = recorded_observation(&write).expect("an observation");
    let category = observed_default_instance(&observation, CATEGORY_IRI);
    assert_eq!(category[ACCURACY_IRI][0][properties::VALUE.as_str()][0]["@value"], json!("high"));
}

/// On the asynchronous path the observation is deferred to the side-effect
/// job, and it is the same resulting instance the inline path records.
#[tokio::test]
async fn batch_merge_defers_the_merged_instance_when_enqueued() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_batch(
        &service,
        BatchMerge,
        vec![building(&json!({"category": {"type": "Property", "value": "retail"}}))],
        &ContextSpec::core_only(),
    )
    .await;
    service
        .batch_merge(SideEffectPlacement::DeferredOnLive, admitted)
        .await
        .expect("the batch merge lands");

    let write = single_recorded_write(&recorded);
    assert!(
        !write.history.iter().any(HistoryRecord::is_observations),
        "the observation is deferred, not inline"
    );
    assert!(write.side_effects.is_some(), "an enqueued write carries its side-effect job");
    assert_observes_stored_instances(&write, &[CATEGORY_IRI]);
}

/// Clause 5.6.9.4 with `noOverwrite` is an Append Attributes that leaves an
/// existing attribute untouched: only the appended attribute is recorded.
#[tokio::test]
async fn batch_update_without_overwrite_records_only_the_attributes_it_appended() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_batch(
        &service,
        BatchUpdate,
        vec![building(&json!({
            "category": {"type": "Property", "value": "retail"},
            "name": {"type": "Property", "value": "lobby"}
        }))],
        &ContextSpec::core_only(),
    )
    .await;
    service
        .batch_update(SideEffectPlacement::Inline(&live_target()), admitted, OverwritePolicy::Preserve)
        .await
        .expect("the batch update lands");

    let write = single_recorded_write(&recorded);
    assert_observes_stored_instances(&write, &[NAME_IRI]);
}

/// A `noOverwrite` batch update that appended nothing records no observation,
/// so the current-value projection keeps the value the Entity still holds.
#[tokio::test]
async fn batch_update_without_overwrite_that_appended_nothing_records_no_observation() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_batch(
        &service,
        BatchUpdate,
        vec![building(&json!({"category": {"type": "Property", "value": "retail"}}))],
        &ContextSpec::core_only(),
    )
    .await;
    service
        .batch_update(SideEffectPlacement::Inline(&live_target()), admitted, OverwritePolicy::Preserve)
        .await
        .expect("the batch update completes");

    let write = single_recorded_write(&recorded);
    assert!(recorded_observation(&write).is_none(), "the skipped value is not recorded: {:?}", write.history);
}

/// Clause 5.6.9 with overwriting records the stored whole instance.
#[tokio::test]
async fn batch_update_with_overwrite_records_the_stored_instance() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_batch(
        &service,
        BatchUpdate,
        vec![building(&json!({"category": {"type": "Property", "value": "retail"}}))],
        &ContextSpec::core_only(),
    )
    .await;
    service
        .batch_update(SideEffectPlacement::Inline(&live_target()), admitted, OverwritePolicy::Overwrite)
        .await
        .expect("the batch update lands");

    let write = single_recorded_write(&recorded);
    assert_observes_stored_instances(&write, &[CATEGORY_IRI]);
}

/// Clause 5.6.8.4 update mode applies Update Attributes semantics.
#[tokio::test]
async fn batch_upsert_update_records_the_stored_instance() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_batch(
        &service,
        BatchUpsert(UpsertPolicy::Update),
        vec![building(&json!({"category": {"type": "Property", "value": "retail"}}))],
        &ContextSpec::core_only(),
    )
    .await;
    service
        .batch_upsert(SideEffectPlacement::Inline(&live_target()), admitted)
        .await
        .expect("the batch upsert lands");

    let write = single_recorded_write(&recorded);
    assert_observes_stored_instances(&write, &[CATEGORY_IRI]);
}

/// Clause 5.6.8.4 replace mode replaces the Entity: every attribute of the
/// replacement is recorded as stored.
#[tokio::test]
async fn batch_upsert_replace_records_every_stored_attribute_of_the_replacement() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_batch(
        &service,
        BatchUpsert(UpsertPolicy::Replace),
        vec![building(&json!({
            "category": {"type": "Property", "value": "retail"},
            "name": {"type": "Property", "value": "lobby"}
        }))],
        &ContextSpec::core_only(),
    )
    .await;
    service
        .batch_upsert(SideEffectPlacement::Inline(&live_target()), admitted)
        .await
        .expect("the batch upsert lands");

    let write = single_recorded_write(&recorded);
    assert_observes_stored_instances(&write, &[CATEGORY_IRI, NAME_IRI]);
}
