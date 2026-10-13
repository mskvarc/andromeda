//! Every batch write records, for the Temporal Evolution, the instances it left
//! in each Entity rather than the Fragment that asked for them, exactly as its
//! single-entity sibling does (clauses 5.6.8, 5.6.9 and 5.6.20 run those
//! behaviours per Entity).
//!
//! The fixture Building holds `category` = "office" with an `accuracy`
//! sub-Property, which a Fragment-shaped record would lose.

use crate::{
    admission_fixture::admitted_batch,
    batch::UpsertPolicy,
    history_recording_assertions::{assert_observes_stored_instances, observed_default_instance, recorded_observation},
    notification_recording_harness::{
        ACCURACY_IRI,
        CATEGORY_IRI,
        ENTITY_ID,
        NAME_IRI,
        live_target,
        recording_service,
        recording_service_counting_enqueued,
        single_recorded_write,
    },
    side_effect::SideEffectPlacement,
    side_effect_replay::replay_writes,
    write_kind::{BatchMerge, BatchUpdate, BatchUpsert},
};
use andromeda_context_commit::history_record::HistoryRecord;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::operation::selection::OverwritePolicy;
use andromeda_ngsi_ld_model::{attribute::member_reading::MemberReading, entity::Entity};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

/// A single-entity batch input built from compact attribute members.
fn building<M: MemberReading>(attributes: &Value) -> Entity<M> {
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

/// A respond-async commit records the same resulting instance the inline path
/// records, inline in the write: the commit queues it as an intent, which the
/// Temporal API answers at once (RFC 7240 section 4.1 lets the answer precede
/// the processing, and the store's reads include queued intents).
#[tokio::test]
async fn batch_merge_records_the_merged_instance_in_the_enqueued_commit() {
    let (service, recorded) = recording_service_counting_enqueued().await;
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

    assert_eq!(recorded.enqueued.load(Ordering::Relaxed), 1, "the batch is one respond-async commit");
    let write = single_recorded_write(&recorded.writes);
    assert!(
        write.history.iter().any(HistoryRecord::is_observations),
        "the observation commits with the write"
    );
    assert_observes_stored_instances(&write, &[CATEGORY_IRI]);
}

/// Clause 5.7.2.4 filters the Entities as stored, and `respond-async` answers
/// once the batch is stored: the enqueued write commits the updated state as
/// current state and its history as queued intents, and the job its replay
/// settles holds change events alone, so a replay landing after a later write
/// cannot bring the older value back.
#[tokio::test]
async fn batch_update_enqueued_commits_its_current_state_and_its_replay_leaves_current_state_alone() {
    let (service, recorded) = recording_service_counting_enqueued().await;
    let admitted = admitted_batch(
        &service,
        BatchUpdate,
        vec![building(&json!({"category": {"type": "Property", "value": "retail"}}))],
        &ContextSpec::core_only(),
    )
    .await;
    service
        .batch_update(SideEffectPlacement::DeferredOnLive, admitted, OverwritePolicy::Overwrite)
        .await
        .expect("the batch update lands");

    assert_eq!(recorded.enqueued.load(Ordering::Relaxed), 1);
    let write = single_recorded_write(&recorded.writes);
    assert!(write.entity.is_update(), "the enqueued write updates the stored Entity");
    let stored = serde_json::to_value(write.entity.record().expect("the write stores an entity record").state.as_ref()).expect("the state encodes");
    assert_eq!(
        observed_default_instance(&stored, CATEGORY_IRI)[properties::VALUE.as_str()][0]["@value"],
        json!("retail")
    );
    assert!(write.history.iter().any(HistoryRecord::is_observations));

    let owed = write.subscription_matches;
    let replayed = replay_writes(owed);
    assert!(replayed.iter().all(|write| write.entity.is_none()), "a replay leaves current state alone");
    assert!(replayed.iter().all(|write| write.history.is_empty()), "a replay records no history");
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

/// A `noOverwrite` batch update that appended nothing stores no write at all,
/// so it records no observation and leaves the Entity as it was.
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

    let writes = recorded.lock().expect("recording mutex is not poisoned");
    assert!(writes.is_empty(), "the skipped value is not recorded: {writes:?}");
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
