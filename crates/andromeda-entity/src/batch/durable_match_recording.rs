//! Every batch write records the durable subscription matches its own live
//! notification would produce, with the per-attribute triggers its single-entity
//! sibling carries (clauses 5.6.7 to 5.6.10, reported under clause 5.2.12):
//! one match per entity and subscription, carrying the whole change
//! (clauses 5.5.11.0 and 5.8.6).
//!
//! These tests commit each batch operation against the shared recording harness
//! and read the matches — and, on the asynchronous path, the deferred change
//! events — off the [`AtomicWrite`] the committer captured. They guard two
//! defects the schema overhaul left open: a batch write that recorded no durable
//! match at all, and one that collapsed every trigger to `entityUpdated` -- and
//! a third: a write that recorded one match per raised trigger, notifying the
//! same subscription several times for one change.

use crate::{
    admission_fixture::admitted_batch,
    batch::UpsertPolicy,
    building_subscription_fixture::subscription_key,
    notification_recording_harness::{
        ACCURACY_IRI,
        CATEGORY_IRI,
        ENTITY_ID,
        attribute_name,
        entity_uri,
        live_recording_service,
        live_target,
        recording_service,
        single_recorded_write,
    },
    side_effect::SideEffectPlacement,
    write_kind::{BatchCreate, BatchMerge, BatchUpdate, BatchUpsert},
};
use andromeda_context_commit::{atomic_write::AtomicWrite, subscription_match::SubscriptionMatch};
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

/// The one match the write owes the fixture subscription (clause 5.8.6).
fn single_match(write: &AtomicWrite) -> &SubscriptionMatch {
    assert_eq!(
        write.subscription_matches.len(),
        1,
        "one matching subscription is owed one notification per change"
    );
    &write.subscription_matches[0]
}

/// Assert the write carries exactly one match against the fixture
/// subscription, raising `triggers` over the one `changed` attribute.
fn assert_match(write: &AtomicWrite, triggers: &[NotificationTrigger], changed: &str) {
    let matched = single_match(write);
    assert_eq!(matched.subscription_key, subscription_key());
    assert_eq!(matched.change.triggers(), triggers);
    assert_eq!(matched.change.attributes(), vec![&attribute_name(changed)]);
    assert!(matched.entity_snapshot.is_some(), "the durable match carries the post-mutation snapshot");
}

#[tokio::test]
async fn batch_create_records_an_entity_created_match_inline() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_batch(&service, BatchCreate, vec![building_entity("office")], &ContextSpec::core_only()).await;
    service
        .batch_create(SideEffectPlacement::Inline(&live_target()), admitted, &ContextSpec::core_only())
        .await
        .expect("the batch create lands");

    let write = single_recorded_write(&recorded);
    assert_match(
        &write,
        &[NotificationTrigger::EntityCreated, NotificationTrigger::AttributeCreated],
        CATEGORY_IRI,
    );
    assert!(write.side_effects.is_none(), "an inline write defers no job");
}

#[tokio::test]
async fn batch_create_records_the_match_and_defers_no_event_when_enqueued() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_batch(&service, BatchCreate, vec![building_entity("office")], &ContextSpec::core_only()).await;
    service
        .batch_create(SideEffectPlacement::DeferredOnLive, admitted, &ContextSpec::core_only())
        .await
        .expect("the batch create lands");

    let write = single_recorded_write(&recorded);
    // The durable match commits with the entity row even on the asynchronous
    // path, so the deferred job owes no notification: carrying the change as
    // well would notify it again when a live-delivery broker drains the job.
    assert_match(
        &write,
        &[NotificationTrigger::EntityCreated, NotificationTrigger::AttributeCreated],
        CATEGORY_IRI,
    );
    let job = write.side_effects.as_ref().expect("an enqueued write carries a deferred job");
    assert_eq!(job.temporal_entities.len(), 1, "the observation is still deferred to the worker");
    assert!(job.change_events.is_empty(), "the notifications were queued at commit");
}

#[tokio::test]
async fn a_live_delivery_batch_create_defers_its_changes_with_the_resulting_entity_when_enqueued() {
    let (service, recorded) = live_recording_service();
    let admitted = admitted_batch(&service, BatchCreate, vec![building_entity("office")], &ContextSpec::core_only()).await;
    service
        .batch_create(SideEffectPlacement::DeferredOnLive, admitted, &ContextSpec::core_only())
        .await
        .expect("the batch create lands");

    let write = single_recorded_write(&recorded);
    assert!(write.subscription_matches.is_empty(), "a live-delivery broker records no durable match");
    let job = write.side_effects.as_ref().expect("an enqueued write carries a deferred job");
    let deferred: Vec<Vec<NotificationTrigger>> = job.change_events.iter().map(|event| event.change.triggers()).collect();
    assert_eq!(
        deferred,
        vec![vec![NotificationTrigger::EntityCreated, NotificationTrigger::AttributeCreated]],
        "one write defers one change event, raising every trigger it owes"
    );
    assert!(
        job.change_events.iter().all(|event| event.current_payload.is_some()),
        "the owed change carries the entity the write left behind"
    );
}

#[tokio::test]
async fn batch_update_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_batch(&service, BatchUpdate, vec![building_entity("retail")], &ContextSpec::core_only()).await;
    service
        .batch_update(
            SideEffectPlacement::Inline(&live_target()),
            admitted,
            &ContextSpec::core_only(),
            OverwritePolicy::Overwrite,
        )
        .await
        .expect("the batch update lands");

    let write = single_recorded_write(&recorded);
    // The overwritten attribute already existed, so the batch update raises
    // attributeUpdated, not the coarse entityUpdated the defect produced.
    assert_match(&write, &[NotificationTrigger::AttributeUpdated], CATEGORY_IRI);
}

#[tokio::test]
async fn batch_merge_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_batch(&service, BatchMerge, vec![building_entity("retail")], &ContextSpec::core_only()).await;
    service
        .batch_merge(SideEffectPlacement::Inline(&live_target()), admitted, &ContextSpec::core_only())
        .await
        .expect("the batch merge lands");

    let write = single_recorded_write(&recorded);
    assert_match(&write, &[NotificationTrigger::AttributeUpdated], CATEGORY_IRI);
}

#[tokio::test]
async fn batch_upsert_replace_of_an_existing_entity_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_batch(
        &service,
        BatchUpsert(UpsertPolicy::Replace),
        vec![building_entity("retail")],
        &ContextSpec::core_only(),
    )
    .await;
    service
        .batch_upsert(SideEffectPlacement::Inline(&live_target()), admitted, &ContextSpec::core_only())
        .await
        .expect("the batch upsert lands");

    let write = single_recorded_write(&recorded);
    assert_match(&write, &[NotificationTrigger::AttributeUpdated], CATEGORY_IRI);
}

#[tokio::test]
async fn batch_upsert_update_of_an_existing_entity_records_an_attribute_updated_match() {
    let (service, recorded) = recording_service().await;
    let admitted = admitted_batch(
        &service,
        BatchUpsert(UpsertPolicy::Update),
        vec![building_entity("retail")],
        &ContextSpec::core_only(),
    )
    .await;
    service
        .batch_upsert(SideEffectPlacement::Inline(&live_target()), admitted, &ContextSpec::core_only())
        .await
        .expect("the batch upsert lands");

    let write = single_recorded_write(&recorded);
    assert_match(&write, &[NotificationTrigger::AttributeUpdated], CATEGORY_IRI);
}

#[tokio::test]
async fn batch_upsert_update_replaces_the_whole_instance_dropping_a_subattribute() {
    // Clause 5.6.8.4: upsert update mode applies Update Attributes semantics, so
    // it overwrites the whole `category` instance and drops the stored `accuracy`
    // sub-attribute the request omits, matching the single Update Attributes path.
    let (service, recorded) = recording_service().await;
    let admitted = admitted_batch(
        &service,
        BatchUpsert(UpsertPolicy::Update),
        vec![building_entity("retail")],
        &ContextSpec::core_only(),
    )
    .await;
    service
        .batch_upsert(SideEffectPlacement::Inline(&live_target()), admitted, &ContextSpec::core_only())
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
        .batch_delete(SideEffectPlacement::Inline(&live_target()), &[entity_uri()])
        .await
        .expect("the batch delete completes");

    let write = single_recorded_write(&recorded);
    // Clause 5.6.10: one match, raising entityDeleted together with the
    // attributeDeleted of every attribute the entity took with it.
    assert_match(
        &write,
        &[NotificationTrigger::EntityDeleted, NotificationTrigger::AttributeDeleted],
        CATEGORY_IRI,
    );
}

#[tokio::test]
async fn batch_delete_records_matches_and_defers_nothing_when_enqueued() {
    let (service, recorded) = recording_service().await;
    service
        .batch_delete(SideEffectPlacement::DeferredOnLive, &[entity_uri()])
        .await
        .expect("the batch delete completes");

    let write = single_recorded_write(&recorded);
    // The durable match commits with the delete even on the asynchronous path.
    assert_match(
        &write,
        &[NotificationTrigger::EntityDeleted, NotificationTrigger::AttributeDeleted],
        CATEGORY_IRI,
    );
    assert!(write.side_effects.is_none(), "a delete whose notifications are queued at commit owes no job");
}

#[tokio::test]
async fn a_live_delivery_batch_delete_defers_its_events_when_enqueued() {
    let (service, recorded) = live_recording_service();
    service
        .batch_delete(SideEffectPlacement::DeferredOnLive, &[entity_uri()])
        .await
        .expect("the batch delete completes");

    let write = single_recorded_write(&recorded);
    assert!(write.subscription_matches.is_empty(), "a live-delivery broker records no durable match");
    let job = write.side_effects.as_ref().expect("an enqueued delete carries a deferred job");
    let deferred: Vec<Vec<NotificationTrigger>> = job.change_events.iter().map(|event| event.change.triggers()).collect();
    assert_eq!(deferred, vec![vec![NotificationTrigger::EntityDeleted, NotificationTrigger::AttributeDeleted]]);
    assert!(
        job.change_events
            .iter()
            .all(|event| event.entity_snapshot.is_some() && event.sys_attrs_snapshot.is_some()),
        "a deferred deletion carries what was removed and its system attributes"
    );
}
