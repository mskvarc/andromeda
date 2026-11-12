//! A database that never stored a seal upgrades with nothing changed: no row
//! is added or removed, and no element, search row or ledger moves.

use crate::{
    census_ledger::{count_instance, count_stream, instances_by_attribute, instances_by_type_and_attribute, streams_by_type_and_attribute},
    current_state::{insert_entity, insert_entity_search, insert_number_attribute, insert_type_member, intern_attribute},
    history::{InstanceIdentity, insert_history_row, insert_stream},
    queues::{entity_state, insert_append_intent, insert_notification_event, insert_pending_change, temporal_entry},
    rehearsal::at_v1_0_0,
    statements::{count_rows, read_column, read_documents},
    test_error::UpgradeTestError,
    vocabulary::string_element,
};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::collections::BTreeMap;

/// The relations the seeded rows live in.
const TABLES: [&str; 15] = [
    "attribute_names",
    "entity_type_names",
    "entities",
    "entity_search",
    "entity_type_members",
    "current_attributes",
    "attribute_census",
    "entity_type_attribute_census",
    "stream_type_census",
    "temporal_attribute_streams",
    "attribute_history",
    "attribute_history_instance_claims",
    "pending_side_effects",
    "notification_events",
    "temporal_append_intents",
];

/// What the upgrade must leave as it found it.
#[derive(Debug, PartialEq)]
struct Observation {
    /// How many rows each relation holds.
    row_counts: Vec<(&'static str, i64)>,
    /// The members of every sub-attribute of the current state.
    current_elements: Vec<Value>,
    /// The members of every sub-attribute of the history.
    history_elements: Vec<Value>,
    /// The members of every sub-attribute the queues hold.
    queued_elements: Vec<Value>,
    /// The attribute names each search row lists.
    searched_names: Vec<Vec<i32>>,
    /// The instances each attribute holds, per the census.
    instances: BTreeMap<i32, i64>,
    /// The instances each attribute holds under each type, per the census.
    typed_instances: BTreeMap<(i32, i32), i64>,
    /// The streams each attribute holds under each type, per the census.
    typed_streams: BTreeMap<(i32, i32), i64>,
}

/// The members of a sub-attribute that the upgrade leaves as they were.
fn kept_members(element: &Value) -> Value {
    json!({
        "parent": element["parent"],
        "attribute_name_key": element["attribute_name_key"],
        "attribute_kind": element["attribute_kind"],
        "value_kind": element["value_kind"],
        "value_string": element["value_string"]
    })
}

/// The kept members of every element of the JSON arrays `columns`.
fn element_members<'column>(columns: impl IntoIterator<Item = &'column Value>) -> Vec<Value> {
    columns.into_iter().filter_map(Value::as_array).flatten().map(kept_members).collect()
}

/// The kept members of every sub-attribute of the Entity states and append
/// entries the queues hold.
async fn queued_elements(pool: &PgPool) -> Result<Vec<Value>, UpgradeTestError> {
    let events = read_documents(pool, "pending_side_effects", "events", "recorded_at", None).await?;
    let snapshots = read_documents(pool, "notification_events", "entity_snapshot", "event_key", None).await?;
    let entries = read_documents(pool, "temporal_append_intents", "entries", "intent_key", None).await?;
    let pending_states = events
        .iter()
        .flatten()
        .filter_map(Value::as_array)
        .flatten()
        .map(|event| &event["entity_snapshot"]);
    let states = pending_states.chain(snapshots.iter().flatten());
    let state_elements = states.flat_map(|state| {
        let instances = state["instances"].as_array().into_iter().flatten();
        element_members(instances.map(|instance| &instance["sub_attributes"]))
    });
    let entry_elements = element_members(
        entries
            .iter()
            .flatten()
            .filter_map(Value::as_array)
            .flatten()
            .map(|entry| &entry["sub_attributes"]),
    );
    Ok(state_elements.chain(entry_elements).collect())
}

/// Reads everything the upgrade must leave unchanged.
async fn observe(pool: &PgPool) -> Result<Observation, UpgradeTestError> {
    let mut row_counts = Vec::new();
    for table in TABLES {
        row_counts.push((table, count_rows(pool, table).await?));
    }
    let current = read_documents(pool, "current_attributes", "sub_attributes", "attribute_name_key", None).await?;
    let history = read_documents(pool, "attribute_history", "sub_attributes", "stream_key", None).await?;
    Ok(Observation {
        row_counts,
        current_elements: element_members(current.iter().flatten()),
        history_elements: element_members(history.iter().flatten()),
        queued_elements: queued_elements(pool).await?,
        searched_names: read_column::<Vec<i32>>(pool, "entity_search", "attribute_name_keys").await?,
        instances: instances_by_attribute(pool).await?,
        typed_instances: instances_by_type_and_attribute(pool).await?,
        typed_streams: streams_by_type_and_attribute(pool).await?,
    })
}

#[tokio::test]
async fn a_database_holding_no_seal_upgrades_with_nothing_changed() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let note_key = intern_attribute(database.pool(), "https://example.org/note").await?;
    let entity_key = insert_entity(database.pool(), "urn:ngsi-ld:Store:001").await?;
    insert_type_member(database.pool(), entity_key, keys.store, "urn:ngsi-ld:Store:001").await?;
    insert_entity_search(database.pool(), entity_key, &[keys.store], &[keys.temperature]).await?;
    let elements = [string_element(0, note_key, "plain")];
    insert_number_attribute(database.pool(), entity_key, keys.temperature, 0, &elements).await?;
    count_instance(database.pool(), keys.store, keys.temperature).await?;
    let stream = insert_stream(database.pool(), entity_key, keys.temperature).await?;
    count_stream(database.pool(), keys.store, keys.temperature).await?;
    insert_history_row(
        database.pool(),
        stream,
        &InstanceIdentity::Generated("00000000-0000-4000-8000-0000000000f1"),
        &elements,
    )
    .await?;
    let state = entity_state(keys.store, keys.temperature, &elements);
    insert_pending_change(database.pool(), keys.store, &state).await?;
    insert_notification_event(database.pool(), &state).await?;
    insert_append_intent(database.pool(), keys.store, &[temporal_entry(keys.temperature, &elements)]).await?;
    let before = observe(database.pool()).await?;

    database.migrate_to_current().await?;

    assert_eq!(observe(database.pool()).await?, before);
    database.drop_database().await?;
    Ok(())
}
