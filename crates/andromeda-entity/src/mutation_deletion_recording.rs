//! Delete Entity (clause 5.6.6) reads the state it removes only when a
//! subscription may be owed the deletion's notification.
//!
//! These tests delete the harness Building under every combination of what the
//! shared store's watcher probe answers and what this process's subscription
//! cache holds, and read off the recording committer, the payload-read count
//! and the change sink what the deletion did: whether it read the payload, the
//! matches it recorded (clause 5.8.6), the tombstones it recorded (clauses
//! 4.5.7 and 4.5.8) and the change it published.

use crate::{
    deletion_outline::DeletionWatchers,
    deletion_outline_fixture::ProbeAnswer,
    notification_recording_harness::{
        CATEGORY_IRI,
        CachedSubscription,
        ObservedService,
        assert_matches_share_their_snapshots,
        assert_single_match,
        attribute_name,
        entity_uri,
        live_target,
        probing_live_service,
        probing_recording_service,
        single_recorded_write,
    },
};
use andromeda_context_commit::{change_event::ChangeEvent, history_record::HistoryRecord};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_model::{
    entity_change::{EntityChange, EntityLifecycle},
    expanded::{attribute_change::AttributeChangeKind, instance_key::AttributeInstanceKey, removed_instance::RemovedAttributeInstance},
};
use pretty_assertions::assert_eq;
use std::sync::{Arc, Mutex, atomic::Ordering};

/// Delete the harness Building through a service whose probe answers `probe`
/// and whose cache holds `cached`.
async fn deleted_through(probe: ProbeAnswer, cached: CachedSubscription) -> ObservedService {
    let observed = probing_recording_service(probe, cached).await;
    observed.service.delete(&live_target(), &entity_uri()).await.expect("the deletion lands");
    observed
}

/// Take the single change the deletion published.
fn single_published(published: &Arc<Mutex<Vec<ChangeEvent>>>) -> ChangeEvent {
    let mut events = published.lock().expect("recording mutex is not poisoned");
    assert_eq!(events.len(), 1, "one deletion publishes one change");
    events.remove(0)
}

/// The change deleting the harness Building is: its one `category` instance
/// taken with it.
fn assert_building_deletion(change: &EntityChange) {
    assert_eq!(change.lifecycle, EntityLifecycle::Deleted);
    let instances: Vec<_> = change.instances.iter().map(|instance| (instance.kind, &instance.key.attr)).collect();
    assert_eq!(instances, vec![(AttributeChangeKind::Deleted, &attribute_name(CATEGORY_IRI))]);
}

#[tokio::test]
async fn a_deletion_no_subscription_watches_never_reads_the_removed_payload() {
    let observed = deleted_through(ProbeAnswer::Answers(DeletionWatchers::None), CachedSubscription::None).await;

    assert_eq!(observed.payload_reads.load(Ordering::Relaxed), 0);
    let write = single_recorded_write(&observed.recorded);
    assert!(write.subscription_matches.is_empty());
    let event = single_published(&observed.published);
    assert_building_deletion(&event.change);
    assert!(event.entity_snapshot.is_none() && event.previous_attributes.is_none());
}

/// Clauses 4.5.7 and 4.5.8: the Building's one default-dataset `category`
/// Property ends with it, and it held no Scope to end. Which tombstones an
/// outline of many instances, datasets and a Scope records is proven where
/// the tombstones are built, against the removed payload.
#[tokio::test]
async fn a_deletion_no_subscription_watches_records_the_tombstones_of_its_outline() {
    let observed = deleted_through(ProbeAnswer::Answers(DeletionWatchers::None), CachedSubscription::None).await;

    let write = single_recorded_write(&observed.recorded);
    let [HistoryRecord::Tombstone(ended)] = write.history.as_slice() else {
        panic!("one tombstone and no Scope removal: {:?}", write.history);
    };
    let expected = RemovedAttributeInstance {
        key: AttributeInstanceKey {
            attr: attribute_name(CATEGORY_IRI),
            dataset_id: None,
        },
        attribute_type: AttributeType::Property,
    };
    assert_eq!(ended, &expected);
}

#[tokio::test]
async fn a_deletion_reads_the_removed_payload_when_only_the_store_knows_a_watcher() {
    let observed = deleted_through(ProbeAnswer::Answers(DeletionWatchers::Possible), CachedSubscription::None).await;

    assert_eq!(observed.payload_reads.load(Ordering::Relaxed), 1);
    // The cache of this replica holds no subscription yet, so nothing is
    // recorded here; the replica that learnt the subscription is notified
    // through the published change, which carries the removed state.
    let write = single_recorded_write(&observed.recorded);
    assert!(write.subscription_matches.is_empty());
    let event = single_published(&observed.published);
    assert_building_deletion(&event.change);
    assert!(event.entity_snapshot.is_some());
}

#[tokio::test]
async fn a_deletion_reads_the_removed_payload_when_only_this_process_caches_a_watcher() {
    let observed = deleted_through(ProbeAnswer::Answers(DeletionWatchers::None), CachedSubscription::Building).await;

    assert_eq!(observed.payload_reads.load(Ordering::Relaxed), 1);
    let write = single_recorded_write(&observed.recorded);
    assert_single_match(&write, &[(AttributeChangeKind::Deleted, CATEGORY_IRI)]);
}

/// An empty cache would answer that no subscription watches, so only the
/// failed probe makes this deletion read the state it removes.
#[tokio::test]
async fn a_failed_watcher_probe_reads_the_removed_payload() {
    let observed = deleted_through(ProbeAnswer::Fails, CachedSubscription::None).await;

    assert_eq!(observed.payload_reads.load(Ordering::Relaxed), 1);
    let event = single_published(&observed.published);
    assert_building_deletion(&event.change);
    assert!(event.entity_snapshot.is_some(), "the published change carries the removed state");
}

/// Table 5.2.12-1: `entityUpdated` includes `attributeDeleted`, so a
/// subscription listening for it alone is owed the deletion of the Building's
/// instances. A live-delivery broker records no match at commit, and its
/// notification worker renders the removed state the published change
/// carries; the cache answers for the subscription however the store's probe
/// answered.
#[tokio::test]
async fn a_live_delivery_deletion_reads_the_removed_payload_for_an_entity_updated_subscription() {
    let observed = probing_live_service(ProbeAnswer::Answers(DeletionWatchers::None), CachedSubscription::BuildingOnEntityUpdated).await;
    observed.service.delete(&live_target(), &entity_uri()).await.expect("the deletion lands");

    assert_eq!(observed.payload_reads.load(Ordering::Relaxed), 1);
    assert!(single_recorded_write(&observed.recorded).subscription_matches.is_empty());
    let event = single_published(&observed.published);
    assert_building_deletion(&event.change);
    assert!(event.entity_snapshot.is_some(), "the published change carries the removed state");
}

#[tokio::test]
async fn a_live_delivery_deletion_no_subscription_watches_never_reads_the_removed_payload() {
    let observed = probing_live_service(ProbeAnswer::Answers(DeletionWatchers::None), CachedSubscription::None).await;
    observed.service.delete(&live_target(), &entity_uri()).await.expect("the deletion lands");

    assert_eq!(observed.payload_reads.load(Ordering::Relaxed), 0);
    assert!(single_published(&observed.published).entity_snapshot.is_none());
}

#[tokio::test]
async fn a_durable_deletion_records_the_match_of_an_entity_updated_subscription() {
    let observed = deleted_through(ProbeAnswer::Answers(DeletionWatchers::None), CachedSubscription::BuildingOnEntityUpdated).await;

    assert_eq!(observed.payload_reads.load(Ordering::Relaxed), 1);
    assert_single_match(&single_recorded_write(&observed.recorded), &[(AttributeChangeKind::Deleted, CATEGORY_IRI)]);
}

#[tokio::test]
async fn both_deletion_paths_publish_the_same_change() {
    let outlined = deleted_through(ProbeAnswer::Answers(DeletionWatchers::None), CachedSubscription::None).await;
    let removed = deleted_through(ProbeAnswer::Answers(DeletionWatchers::Possible), CachedSubscription::Building).await;

    assert_eq!(single_published(&outlined.published).change, single_published(&removed.published).change);
}

#[tokio::test]
async fn a_deletion_publishes_one_snapshot_for_both_event_fields() {
    let observed = deleted_through(ProbeAnswer::Answers(DeletionWatchers::Possible), CachedSubscription::Building).await;

    let event = single_published(&observed.published);
    let (Some(snapshot), Some(previous)) = (&event.entity_snapshot, &event.previous_attributes) else {
        panic!("a deletion read for its watchers publishes the removed state");
    };
    assert!(Arc::ptr_eq(snapshot, previous));
}

#[tokio::test]
async fn the_recorded_match_shares_the_removed_state_as_its_previous_snapshot() {
    let observed = deleted_through(ProbeAnswer::Answers(DeletionWatchers::Possible), CachedSubscription::Building).await;

    let write = single_recorded_write(&observed.recorded);
    let event = single_published(&observed.published);
    let (Some(previous), Some(removed)) = (&write.subscription_matches[0].previous_snapshot, &event.previous_attributes) else {
        panic!("the match and the event both carry the removed state");
    };
    assert!(Arc::ptr_eq(previous, removed));
}

#[tokio::test]
async fn every_match_of_one_deletion_shares_one_removed_state() {
    let observed = deleted_through(ProbeAnswer::Answers(DeletionWatchers::Possible), CachedSubscription::BuildingTwice).await;

    assert_matches_share_their_snapshots(&single_recorded_write(&observed.recorded));
}

/// By delivery time the deleted row is gone, so the durable match stores the
/// removed state with its clause 4.8 system attributes already embedded
/// (clause 5.8.6), while the previous state stays the removed state as read.
#[tokio::test]
async fn a_deletion_durable_match_still_carries_the_removed_system_attributes() {
    let observed = deleted_through(ProbeAnswer::Answers(DeletionWatchers::Possible), CachedSubscription::Building).await;

    let write = single_recorded_write(&observed.recorded);
    let matched = &write.subscription_matches[0];
    let (Some(notified), Some(previous)) = (&matched.entity_snapshot, &matched.previous_snapshot) else {
        panic!("the match carries the removed state");
    };
    let notified_text = notified.to_string();
    assert!(notified_text.contains("createdAt") && notified_text.contains("modifiedAt"), "{notified_text}");
    assert!(!previous.to_string().contains("createdAt"), "{previous}");
}
