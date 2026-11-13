//! The three queues of a tenant as v1.0.0 wrote them: the changes a
//! respond-async job owes, the notification events, and the temporal append
//! intents, each holding Entity states or entries that carry sub-attributes.

use crate::fixtures::{
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
    entity_state_of(type_key, &[number_instance(attribute_key, 0, sub_attributes)])
}

/// The state of an Entity of `type_key` holding `instances`.
pub(crate) fn entity_state_of(type_key: i32, instances: &[Value]) -> Value {
    json!({
        "entity_type_name_keys": [type_key],
        "scope_paths": [],
        "instances": instances
    })
}

/// A Property instance of `attribute_key` at `ordinal` holding the number 21
/// beside `sub_attributes`.
pub(crate) fn number_instance(attribute_key: i32, ordinal: i32, sub_attributes: &[Value]) -> Value {
    json!({
        "created_at": QUEUED_AT,
        "modified_at": QUEUED_AT,
        "attribute_name_key": attribute_key,
        "ordinal": ordinal,
        "attribute_kind": 1,
        "value_kind": 1,
        "value_number": 21,
        "dataset_key": "",
        "sub_attributes": sub_attributes
    })
}

/// A top-level seal instance at `ordinal`, which signs no attribute of the
/// Entity: an `ngsildproof` Property whose object value is `proof`, beside
/// `subproperties`.
pub(crate) fn seal_instance(seal_key: i32, ordinal: i32, proof: &Value, subproperties: &Value) -> Value {
    json!({
        "created_at": QUEUED_AT,
        "modified_at": QUEUED_AT,
        "attribute_name_key": seal_key,
        "ordinal": ordinal,
        "attribute_kind": 1,
        "value_kind": 5,
        "value_object": proof,
        "dataset_key": "",
        "proof_seals": subproperties
    })
}

/// One Attribute instance a queued change touched.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ChangedInstance<'a> {
    /// The transition: 1 created, 2 updated, 3 deleted.
    pub(crate) kind: i16,
    /// The `attribute_names` key of the instance's attribute.
    pub(crate) attribute_key: i32,
    /// The instance's `datasetId`, `None` for the default instance.
    pub(crate) dataset_id: Option<&'a str>,
}

/// A change of the Entity as a queue holds it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct QueuedChange<'a> {
    /// The Entity's transition: 1 created, 2 modified, 3 deleted.
    pub(crate) lifecycle: i16,
    /// The state the change left the Entity in.
    pub(crate) state: &'a Value,
    /// The state before the change, when the change left the Entity in place.
    pub(crate) previous: Option<&'a Value>,
    /// The instances the change touched, in recorded order.
    pub(crate) instances: &'a [ChangedInstance<'a>],
}

impl<'a> QueuedChange<'a> {
    /// The creation of the Entity in the state `state`, touching no instance.
    pub(crate) const fn creating(state: &'a Value) -> QueuedChange<'a> {
        QueuedChange {
            lifecycle: 1,
            state,
            previous: None,
            instances: &[],
        }
    }

    /// The transitions of the instances the change touched.
    fn kinds(&self) -> Vec<i16> {
        self.instances.iter().map(|instance| instance.kind).collect()
    }

    /// The `attribute_names` keys of the instances the change touched.
    fn attribute_keys(&self) -> Vec<i32> {
        self.instances.iter().map(|instance| instance.attribute_key).collect()
    }

    /// The `datasetId`s of the instances the change touched.
    fn dataset_ids(&self) -> Vec<Option<&'a str>> {
        self.instances.iter().map(|instance| instance.dataset_id).collect()
    }
}

/// Queues `change` of the Entity for a respond-async job.
pub(crate) async fn insert_pending_change(pool: &PgPool, type_key: i32, change: &QueuedChange<'_>) -> Result<(), UpgradeTestError> {
    let event = json!({
        "lifecycle": change.lifecycle,
        "entity_id": ENTITY_ID,
        "entity_type_name_keys": [type_key],
        "change_kinds": change.kinds(),
        "change_attribute_name_keys": change.attribute_keys(),
        "change_dataset_ids": change.dataset_ids(),
        "occurred_at": QUEUED_AT,
        "origin": 1,
        "entity_snapshot": change.state,
        "previous_snapshot": change.previous,
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

/// Records a notification event for `change` of the Entity.
pub(crate) async fn insert_notification_event(pool: &PgPool, change: &QueuedChange<'_>) -> Result<(), UpgradeTestError> {
    let mut row = vec![
        ("created_at", instant(QUEUED_AT)),
        ("committed_at", instant(QUEUED_AT)),
        ("event_ordinal", Expr::val(0_i32)),
        ("lifecycle", smallint(change.lifecycle)),
        ("write_id", uuid("00000000-0000-4000-8000-0000000000c1")),
        ("entity_id", Expr::val(ENTITY_ID)),
        ("change_kinds", smallints(&change.kinds())),
        ("change_attribute_name_keys", integers(&change.attribute_keys())),
        ("change_dataset_ids", texts(&change.dataset_ids())),
        ("entity_snapshot", composite(PostgresType::EntityStateRecord, change.state)),
    ];
    if let Some(previous) = change.previous {
        row.push(("previous_snapshot", composite(PostgresType::EntityStateRecord, previous)));
    }
    insert(pool, "notification_events", row).await
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
