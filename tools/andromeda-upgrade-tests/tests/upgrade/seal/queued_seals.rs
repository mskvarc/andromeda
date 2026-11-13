//! Seals held in history rows and in the three queues are converted where
//! they stand, and a top-level seal entry of an append intent is dropped.

use crate::fixtures::{
    current_state::insert_entity,
    history::{InstanceIdentity, insert_history_row, insert_stream},
    observed::{assert_typed_seal, single},
    queues::{entity_state, insert_append_intent, insert_notification_event, insert_pending_change, temporal_entry, top_level_seal_entry},
    rehearsal::{at_v1_0_0, intern_seal_attribute},
    statements::read_documents,
    test_error::UpgradeTestError,
    vocabulary::{complete_seals, proof_node, seal_element},
};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// The document a read returned, which a test expects to exist.
fn present(document: Option<Value>, what: &'static str) -> Result<Value, UpgradeTestError> {
    document.ok_or(UpgradeTestError::Missing { what })
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
    insert_pending_change(database.pool(), keys.store, &entity_state(keys.store, keys.temperature, &[seal])).await?;

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
    insert_notification_event(database.pool(), &entity_state(keys.store, keys.temperature, &[seal])).await?;

    database.migrate_to_current().await?;

    let rows = read_documents(database.pool(), "notification_events", "entity_snapshot", "event_key", None).await?;
    let snapshot = present(single(rows, "the notification event")?, "the event snapshot")?;
    assert_typed_seal(&snapshot["instances"][0]["sub_attributes"][0], seal_key, keys.store);
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_seal_in_an_append_intent_entry_becomes_typed_and_a_top_level_seal_entry_is_dropped() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let seal_key = intern_seal_attribute(database.pool()).await?;
    let seal = seal_element(0, seal_key, &proof_node(), &complete_seals());
    let entries = [
        top_level_seal_entry(seal_key, &proof_node(), &complete_seals()),
        temporal_entry(keys.temperature, &[seal]),
    ];
    insert_append_intent(database.pool(), keys.store, &entries).await?;

    database.migrate_to_current().await?;

    let rows = read_documents(database.pool(), "temporal_append_intents", "entries", "intent_key", None).await?;
    let kept = present(single(rows, "the append intent")?, "the intent entries")?;
    assert_eq!(kept.as_array().map(Vec::len), Some(1));
    assert_eq!(kept[0]["attribute_name_key"], json!(keys.temperature));
    assert_typed_seal(&kept[0]["sub_attributes"][0], seal_key, keys.store);
    database.drop_database().await?;
    Ok(())
}
