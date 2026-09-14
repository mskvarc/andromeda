//! Every batch write records the durable subscription matches its own live
//! notification would produce, with the per-attribute triggers its single-entity
//! sibling carries (clauses 5.6.7 to 5.6.10, reported under clause 5.2.12).
//!
//! These tests commit each batch operation against the shared recording harness
//! and read the matches — and, on the asynchronous path, the deferred change
//! events — off the [`AtomicWrite`] the committer captured. They guard two
//! defects the schema overhaul left open: a batch write that recorded no durable
//! match at all, and one that collapsed every trigger to `entityUpdated`.

use crate::{
    batch::UpsertPolicy,
    notification_recording_harness::{
        ACCURACY_IRI,
        CATEGORY_IRI,
        ENTITY_ID,
        attribute_name,
        entity_uri,
        live_target,
        recording_service,
        single_recorded_write,
        subscription_key,
    },
    side_effect::BatchSideEffectMode,
};
use andromeda_context_commit::atomic_write::AtomicWrite;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::{entity::Entity, expanded::mutation::selection::OverwritePolicy, notification::NotificationTrigger};
use pretty_assertions::assert_eq;
use serde_json::json;

/// A single-entity batch input carrying one `category` Property, in the compact
/// form a request submits.
fn building_entity(value: &str) -> Entity {
    serde_json::from_value(json!({
        "id": ENTITY_ID,
        "type": "Building",
        "category": {"type": "Property", "value": value}
    }))
    .expect("the fixture entity is valid")
}

/// The triggers the write is owed a durable match for, in submission order.
fn match_triggers(write: &AtomicWrite) -> Vec<NotificationTrigger> {
    write.subscription_matches.iter().map(|matched| matched.trigger).collect()
}

/// Assert the write carries exactly one match with the given trigger and changed
/// attribute against the fixture subscription.
fn assert_match(write: &AtomicWrite, trigger: NotificationTrigger, changed: &str) {
    assert_eq!(
        write.subscription_matches.len(),
        1,
        "one matching subscription is owed one notification: {:?}",
        match_triggers(write)
    );
    let matched = &write.subscription_matches[0];
    assert_eq!(matched.subscription_key, subscription_key());
    assert_eq!(matched.trigger, trigger);
    assert_eq!(matched.changed_attrs, vec![attribute_name(changed)]);
    assert!(matched.entity_snapshot.is_some(), "the durable match carries the post-mutation snapshot");
}

#[tokio::test]
async fn batch_create_records_an_entity_created_match_inline() {
    let (service, recorded) = recording_service().await;
    service
        .batch_create(
            &live_target(),
            &[building_entity("office")],
            &ContextSpec::core_only(),
            BatchSideEffectMode::Inline,
        )
        .await
        .expect("the batch create lands");

    let write = single_recorded_write(&recorded);
    assert_eq!(
        match_triggers(&write),
        vec![NotificationTrigger::EntityCreated, NotificationTrigger::AttributeCreated]
    );
    assert!(
        write
            .subscription_matches
            .iter()
            .all(|matched| matched.changed_attrs == vec![attribute_name(CATEGORY_IRI)])
    );
    assert!(write.side_effects.is_none(), "an inline write defers no job");
}

#[tokio::test]
async fn batch_create_records_the_match_and_defers_the_event_when_enqueued() {
    let (service, recorded) = recording_service().await;
    service
        .batch_create(
            &live_target(),
            &[building_entity("office")],
            &ContextSpec::core_only(),
            BatchSideEffectMode::Enqueue,
        )
        .await
        .expect("the batch create lands");

    let write = single_recorded_write(&recorded);
    // The durable match commits with the entity row even on the asynchronous
    // path; only the in-process event is deferred to the worker.
    assert_eq!(
        match_triggers(&write),
        vec![NotificationTrigger::EntityCreated, NotificationTrigger::AttributeCreated]
    );
    let job = write.side_effects.as_ref().expect("an enqueued write carries a deferred job");
    assert_eq!(
        job.change_events.len(),
        2,
        "the lifecycle and attribute-created events are deferred to the worker"
    );
    assert_eq!(job.change_events[0].trigger_reason, NotificationTrigger::EntityCreated);
    assert_eq!(job.change_events[1].trigger_reason, NotificationTrigger::AttributeCreated);
}

#[tokio::test]
async fn batch_update_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    service
        .batch_update(
            &live_target(),
            &[building_entity("retail")],
            &ContextSpec::core_only(),
            OverwritePolicy::Overwrite,
            BatchSideEffectMode::Inline,
        )
        .await
        .expect("the batch update lands");

    let write = single_recorded_write(&recorded);
    // The overwritten attribute already existed, so the batch update raises
    // attributeUpdated, not the coarse entityUpdated the defect produced.
    assert_match(&write, NotificationTrigger::AttributeUpdated, CATEGORY_IRI);
}

#[tokio::test]
async fn batch_merge_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    service
        .batch_merge(
            &live_target(),
            &[building_entity("retail")],
            &ContextSpec::core_only(),
            BatchSideEffectMode::Inline,
        )
        .await
        .expect("the batch merge lands");

    let write = single_recorded_write(&recorded);
    assert_match(&write, NotificationTrigger::AttributeUpdated, CATEGORY_IRI);
}

#[tokio::test]
async fn batch_upsert_replace_of_an_existing_entity_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    service
        .batch_upsert(
            &live_target(),
            &[building_entity("retail")],
            &ContextSpec::core_only(),
            UpsertPolicy::Replace,
            BatchSideEffectMode::Inline,
        )
        .await
        .expect("the batch upsert lands");

    let write = single_recorded_write(&recorded);
    assert_match(&write, NotificationTrigger::AttributeUpdated, CATEGORY_IRI);
}

#[tokio::test]
async fn batch_upsert_update_of_an_existing_entity_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    service
        .batch_upsert(
            &live_target(),
            &[building_entity("retail")],
            &ContextSpec::core_only(),
            UpsertPolicy::Update,
            BatchSideEffectMode::Inline,
        )
        .await
        .expect("the batch upsert lands");

    let write = single_recorded_write(&recorded);
    assert_match(&write, NotificationTrigger::AttributeUpdated, CATEGORY_IRI);
}

#[tokio::test]
async fn batch_upsert_update_replaces_the_whole_instance_dropping_a_subattribute() {
    // Clause 5.6.8.4: upsert update mode applies Update Attributes semantics, so
    // it overwrites the whole `category` instance and drops the stored `accuracy`
    // sub-attribute the request omits, matching the single Update Attributes path.
    let (service, recorded) = recording_service().await;
    service
        .batch_upsert(
            &live_target(),
            &[building_entity("retail")],
            &ContextSpec::core_only(),
            UpsertPolicy::Update,
            BatchSideEffectMode::Inline,
        )
        .await
        .expect("the batch upsert lands");

    let write = single_recorded_write(&recorded);
    let payload = &write.entity.record().expect("an upsert update carries an entity record").payload;
    let category = &payload[CATEGORY_IRI][0];
    assert_eq!(category["https://uri.etsi.org/ngsi-ld/hasValue"][0]["@value"], json!("retail"));
    assert!(
        category.get(ACCURACY_IRI).is_none(),
        "the whole-instance overwrite drops the unmentioned sub-attribute"
    );
}

#[tokio::test]
async fn batch_delete_records_attribute_deleted_and_entity_deleted_matches() {
    let (service, recorded) = recording_service().await;
    service
        .batch_delete(&live_target(), &[entity_uri()], BatchSideEffectMode::Inline)
        .await
        .expect("the batch delete completes");

    let write = single_recorded_write(&recorded);
    // Clause 5.6.10: one attributeDeleted naming every attribute the entity took
    // with it, then one entityDeleted.
    assert_eq!(
        match_triggers(&write),
        vec![NotificationTrigger::AttributeDeleted, NotificationTrigger::EntityDeleted],
        "a batch delete owes both an attributeDeleted and an entityDeleted match"
    );
    assert_eq!(write.subscription_matches[0].changed_attrs, vec![attribute_name(CATEGORY_IRI)]);
}

#[tokio::test]
async fn batch_delete_records_matches_and_defers_events_when_enqueued() {
    let (service, recorded) = recording_service().await;
    service
        .batch_delete(&live_target(), &[entity_uri()], BatchSideEffectMode::Enqueue)
        .await
        .expect("the batch delete completes");

    let write = single_recorded_write(&recorded);
    assert_eq!(
        match_triggers(&write),
        vec![NotificationTrigger::AttributeDeleted, NotificationTrigger::EntityDeleted],
        "the durable matches commit with the delete even on the asynchronous path"
    );
    let job = write.side_effects.as_ref().expect("an enqueued delete carries a deferred job");
    let deferred: Vec<NotificationTrigger> = job.change_events.iter().map(|event| event.trigger_reason).collect();
    assert_eq!(deferred, vec![NotificationTrigger::AttributeDeleted, NotificationTrigger::EntityDeleted]);
}
