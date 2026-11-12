//! The three queues of a tenant as v1.0.0 wrote them: the changes a
//! respond-async job owes, the notification events, and the temporal append
//! intents, each holding Entity states or entries that carry sub-attributes.

use crate::{
    statements::{composite, composites, insert, instant, integers, smallint, smallints, texts, uuid},
    test_error::UpgradeTestError,
};
use andromeda_postgresql_seaquery::pg_type::PostgresType;
use sea_query::Expr;
use serde_json::{Value, json};
use sqlx::PgPool;

/// The Entity the queued rows concern.
const ENTITY_ID: &str = "urn:ngsi-ld:Store:001";

/// The instant every queued row was made at.
const QUEUED_AT: &str = "2025-01-27T21:00:00Z";

/// The state of an Entity of `type_key` holding one Property instance of
/// `attribute_key` beside `sub_attributes`.
pub(crate) fn entity_state(type_key: i32, attribute_key: i32, sub_attributes: &[Value]) -> Value {
    json!({
        "entity_type_name_keys": [type_key],
        "scope_paths": [],
        "instances": [{
            "created_at": QUEUED_AT,
            "modified_at": QUEUED_AT,
            "attribute_name_key": attribute_key,
            "ordinal": 0,
            "attribute_kind": 1,
            "value_kind": 1,
            "value_number": 21,
            "dataset_key": "",
            "sub_attributes": sub_attributes
        }]
    })
}

/// Queues a change of the Entity to the state `state` for a respond-async job.
pub(crate) async fn insert_pending_change(pool: &PgPool, type_key: i32, state: &Value) -> Result<(), UpgradeTestError> {
    let event = json!({
        "lifecycle": 1,
        "entity_id": ENTITY_ID,
        "entity_type_name_keys": [type_key],
        "change_kinds": [],
        "change_attribute_name_keys": [],
        "change_dataset_ids": [],
        "occurred_at": QUEUED_AT,
        "origin": 1,
        "entity_snapshot": state,
        "entity_created_at": QUEUED_AT,
        "entity_modified_at": QUEUED_AT
    });
    insert(
        pool,
        "pending_side_effects",
        vec![
            ("id", uuid("00000000-0000-4000-8000-0000000000b1")),
            ("recorded_at", instant(QUEUED_AT)),
            ("write_id", uuid("00000000-0000-4000-8000-0000000000b2")),
            ("base_ordinal", Expr::val(0_i32)),
            ("events", composites(PostgresType::PendingChangeEvent, &[event])),
        ],
    )
    .await
}

/// Records a notification event for a change of the Entity to the state
/// `state`.
pub(crate) async fn insert_notification_event(pool: &PgPool, state: &Value) -> Result<(), UpgradeTestError> {
    insert(
        pool,
        "notification_events",
        vec![
            ("created_at", instant(QUEUED_AT)),
            ("committed_at", instant(QUEUED_AT)),
            ("event_ordinal", Expr::val(0_i32)),
            ("lifecycle", smallint(1)),
            ("write_id", uuid("00000000-0000-4000-8000-0000000000c1")),
            ("entity_id", Expr::val(ENTITY_ID)),
            ("change_kinds", smallints(&[])),
            ("change_attribute_name_keys", integers(&[])),
            ("change_dataset_ids", texts(&[])),
            ("entity_snapshot", composite(PostgresType::EntityStateRecord, state)),
        ],
    )
    .await
}

/// An observation of the number 21 of `attribute_key` beside `sub_attributes`,
/// as an intent holds it.
pub(crate) fn temporal_entry(attribute_key: i32, sub_attributes: &[Value]) -> Value {
    json!({
        "entry_kind": 1,
        "attribute_name_key": attribute_key,
        "dataset_key": "",
        "attribute_kind": 1,
        "value_kind": 1,
        "observed_at": QUEUED_AT,
        "created_at": QUEUED_AT,
        "modified_at": QUEUED_AT,
        "generated_instance_id": "00000000-0000-4000-8000-0000000000d1",
        "value_number": 21,
        "sub_attributes": sub_attributes
    })
}

/// An observation whose attribute is the seal itself, which signs no
/// attribute of the Entity.
pub(crate) fn top_level_seal_entry(seal_key: i32, proof: &Value, seals: &Value) -> Value {
    json!({
        "entry_kind": 1,
        "attribute_name_key": seal_key,
        "dataset_key": "",
        "attribute_kind": 1,
        "value_kind": 5,
        "observed_at": QUEUED_AT,
        "created_at": QUEUED_AT,
        "modified_at": QUEUED_AT,
        "generated_instance_id": "00000000-0000-4000-8000-0000000000d2",
        "value_object": proof,
        "proof_seals": seals
    })
}

/// Queues an intent to append `entries` to the history of the Entity.
pub(crate) async fn insert_append_intent(pool: &PgPool, type_key: i32, entries: &[Value]) -> Result<(), UpgradeTestError> {
    insert(
        pool,
        "temporal_append_intents",
        vec![
            ("write_id", uuid("00000000-0000-4000-8000-0000000000e1")),
            ("recorded_at", instant(QUEUED_AT)),
            ("base_ordinal", Expr::val(0_i32)),
            ("entity_id", Expr::val(ENTITY_ID)),
            ("entity_type_name_keys", integers(&[type_key])),
            ("entries", composites(PostgresType::TemporalEntry, entries)),
            ("entry_needs", smallints(&[3])),
        ],
    )
    .await
}
