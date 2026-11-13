//! Seals held in history rows and in the three queues are converted where
//! they stand, a top-level seal held in a queue is dropped, and a queued row
//! that holds no seal is left as v1.0.0 wrote it.

use crate::fixtures::{
    current_state::{insert_entity, intern_attribute},
    history::{InstanceIdentity, insert_history_row, insert_stream},
    observed::{assert_typed_seal, field_of_each, single},
    queues::{
        ChangedInstance,
        QueuedChange,
        entity_state,
        entity_state_of,
        insert_append_intent,
        insert_notification_event,
        insert_pending_change,
        number_instance,
        seal_instance,
        temporal_entry,
        top_level_seal_entry,
    },
    rehearsal::{at_v1_0_0, intern_seal_attribute},
    statements::{read_documents, read_row_versions},
    test_error::UpgradeTestError,
    vocabulary::{complete_seals, proof_node, seal_element, string_element},
};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use sqlx::PgPool;

/// The three queues of a tenant.
const QUEUES: [&str; 3] = ["pending_side_effects", "notification_events", "temporal_append_intents"];

/// The document a read returned, which a test expects to exist.
fn present(document: Option<Value>, what: &'static str) -> Result<Value, UpgradeTestError> {
    document.ok_or(UpgradeTestError::Missing { what })
}

/// The row versions of each queue.
async fn queue_row_versions(pool: &PgPool) -> Result<Vec<(&'static str, Vec<String>)>, UpgradeTestError> {
    let mut versions = Vec::new();
    for queue in QUEUES {
        versions.push((queue, read_row_versions(pool, queue).await?));
    }
    Ok(versions)
}

#[tokio::test]
async fn a_seal_in_a_history_row_becomes_a_typed_seal() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let entity_key = insert_entity(database.pool(), "urn:ngsi-ld:Store:001").await?;
    let stream = insert_stream(database.pool(), entity_key, keys.temperature).await?;
    let seal = seal_element(0, seal_key, &proof_node(), &complete_seals());
    insert_history_row(
        database.pool(),
        stream,
        &InstanceIdentity::Generated("00000000-0000-4000-8000-0000000000f1"),
        &[seal],
    )
    .await?;

    database.migrate_to_current().await?;

    let rows = read_documents(database.pool(), "attribute_history", "sub_attributes", "stream_key", None).await?;
    let sub_attributes = present(single(rows, "the history row")?, "the history sub-attributes")?;
    assert_typed_seal(&sub_attributes[0], seal_key, keys.store);
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_seal_in_a_pending_change_snapshot_becomes_a_typed_seal() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let seal = seal_element(0, seal_key, &proof_node(), &complete_seals());
    let state = entity_state(keys.store, keys.temperature, &[seal]);
    insert_pending_change(database.pool(), keys.store, &QueuedChange::creating(&state)).await?;

    database.migrate_to_current().await?;

    let rows = read_documents(database.pool(), "pending_side_effects", "events", "recorded_at", None).await?;
    let events = present(single(rows, "the pending change")?, "the pending events")?;
    assert_typed_seal(&events[0]["entity_snapshot"]["instances"][0]["sub_attributes"][0], seal_key, keys.store);
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_seal_in_a_notification_event_snapshot_becomes_a_typed_seal() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let seal = seal_element(0, seal_key, &proof_node(), &complete_seals());
    let state = entity_state(keys.store, keys.temperature, &[seal]);
    insert_notification_event(database.pool(), &QueuedChange::creating(&state)).await?;

    database.migrate_to_current().await?;

    let rows = read_documents(database.pool(), "notification_events", "entity_snapshot", "event_key", None).await?;
    let snapshot = present(single(rows, "the notification event")?, "the event snapshot")?;
    assert_typed_seal(&snapshot["instances"][0]["sub_attributes"][0], seal_key, keys.store);
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_seal_in_an_append_intent_entry_becomes_a_typed_seal() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let seal = seal_element(0, seal_key, &proof_node(), &complete_seals());
    insert_append_intent(database.pool(), keys.store, &[temporal_entry(keys.temperature, &[seal])]).await?;

    database.migrate_to_current().await?;

    let rows = read_documents(database.pool(), "temporal_append_intents", "entries", "intent_key", None).await?;
    let entries = present(single(rows, "the append intent")?, "the intent entries")?;
    assert_typed_seal(&entries[0]["sub_attributes"][0], seal_key, keys.store);
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_top_level_seal_entry_of_an_append_intent_is_dropped() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let entries = [
        top_level_seal_entry(seal_key, &proof_node(), &complete_seals()),
        temporal_entry(keys.temperature, &[]),
    ];
    insert_append_intent(database.pool(), keys.store, &entries).await?;

    database.migrate_to_current().await?;

    let rows = read_documents(database.pool(), "temporal_append_intents", "entries", "intent_key", None).await?;
    let kept = present(single(rows, "the append intent")?, "the intent entries")?;
    assert_eq!(field_of_each(&kept, "attribute_name_key"), json!([keys.temperature]));
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_top_level_seal_held_only_in_the_previous_state_of_a_queued_change_is_dropped() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let state = entity_state(keys.store, keys.temperature, &[]);
    let previous = entity_state_of(
        keys.store,
        &[
            seal_instance(seal_key, 0, &proof_node(), &complete_seals()),
            number_instance(keys.temperature, 1, &[]),
        ],
    );
    let change = QueuedChange {
        lifecycle: 2,
        previous: Some(&previous),
        ..QueuedChange::creating(&state)
    };
    insert_pending_change(database.pool(), keys.store, &change).await?;
    insert_notification_event(database.pool(), &change).await?;

    database.migrate_to_current().await?;

    let events = read_documents(database.pool(), "pending_side_effects", "events", "recorded_at", None).await?;
    let pending = present(single(events, "the pending change")?, "the pending events")?;
    let snapshots = read_documents(database.pool(), "notification_events", "previous_snapshot", "event_key", None).await?;
    let notified = present(single(snapshots, "the notification event")?, "the previous snapshot")?;
    for previous in [&pending[0]["previous_snapshot"], &notified] {
        assert_eq!(field_of_each(&previous["instances"], "attribute_name_key"), json!([keys.temperature]));
        assert_eq!(field_of_each(&previous["instances"], "ordinal"), json!([0]));
    }
    database.drop_database().await?;
    Ok(())
}

/// The three aligned change arrays of `change`, a queued change or event read
/// as JSON.
fn change_arrays(change: &Value) -> Value {
    json!({
        "change_kinds": change["change_kinds"],
        "change_attribute_name_keys": change["change_attribute_name_keys"],
        "change_dataset_ids": change["change_dataset_ids"]
    })
}

/// The deletion of the seal leaves neither state holding one, so only the
/// change arrays name it.
#[tokio::test]
async fn a_top_level_seal_a_queued_change_touched_leaves_its_change_arrays() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let state = entity_state(keys.store, keys.temperature, &[]);
    let touched = [
        ChangedInstance {
            kind: 3,
            attribute_key: seal_key,
            dataset_id: Some("urn:ngsi-ld:Dataset:seal"),
        },
        ChangedInstance {
            kind: 2,
            attribute_key: keys.temperature,
            dataset_id: None,
        },
    ];
    let change = QueuedChange {
        lifecycle: 2,
        instances: &touched,
        ..QueuedChange::creating(&state)
    };
    insert_pending_change(database.pool(), keys.store, &change).await?;
    insert_notification_event(database.pool(), &change).await?;

    database.migrate_to_current().await?;

    let events = read_documents(database.pool(), "pending_side_effects", "events", "recorded_at", None).await?;
    let pending = present(single(events, "the pending change")?, "the pending events")?;
    let rows = read_documents(database.pool(), "notification_events", "notification_events", "event_key", None).await?;
    let notified = present(single(rows, "the notification event")?, "the notification event row")?;
    let kept = json!({
        "change_kinds": [2],
        "change_attribute_name_keys": [keys.temperature],
        "change_dataset_ids": [null]
    });
    assert_eq!(change_arrays(&pending[0]), kept);
    assert_eq!(change_arrays(&notified), kept);
    database.drop_database().await?;
    Ok(())
}

/// A row's `xmin` names the transaction that wrote its current version, so an
/// update of the row replaces it.
#[tokio::test]
async fn a_queued_row_holding_no_seal_is_not_rewritten() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    intern_seal_attribute(database.pool()).await?;
    let note_key = intern_attribute(database.pool(), "https://example.org/note").await?;
    let elements = [string_element(0, note_key, "plain")];
    let state = entity_state(keys.store, keys.temperature, &elements);
    let updated = [ChangedInstance {
        kind: 2,
        attribute_key: keys.temperature,
        dataset_id: None,
    }];
    let change = QueuedChange {
        lifecycle: 2,
        previous: Some(&state),
        instances: &updated,
        ..QueuedChange::creating(&state)
    };
    insert_pending_change(database.pool(), keys.store, &change).await?;
    insert_notification_event(database.pool(), &change).await?;
    insert_append_intent(database.pool(), keys.store, &[temporal_entry(keys.temperature, &elements)]).await?;
    let before = queue_row_versions(database.pool()).await?;

    database.migrate_to_current().await?;

    assert_eq!(queue_row_versions(database.pool()).await?, before);
    database.drop_database().await?;
    Ok(())
}
