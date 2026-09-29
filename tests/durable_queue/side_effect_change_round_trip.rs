//! A side-effect job carries each write's one change through the queue whole.
//!
//! One write is one change per Entity (clause 5.8.6, clause 5.5.11.0): its
//! lifecycle transition and every Attribute instance it touched, together with
//! the Entity it left behind -- what the notification renders and what its
//! `q`, `geoQ` and `scopeQ` are evaluated against. A live-delivery broker defers
//! that change to its side-effect job; whichever broker drains the job must
//! hand the same change back, instance for instance, not one change per
//! instance and not a change stripped of the instances it deleted.
//!
//! The write is committed with the broker's own committer and the job is
//! drained by the real side-effect drain in live delivery, whose published
//! changes are what its notification worker renders.

use crate::{
    live_side_effect_drain::live_side_effect_drain,
    queue_database::{live_target, queue_database},
    queue_workers::side_effect_worker,
    replay_committer::committer,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    batch_side_effect_job::BatchSideEffectJob,
    change_event::ChangeEvent,
    committer::ContextCommitter,
    entity_mutation::EntityMutation,
};
use andromeda_entity::side_effect_drain::EntitySideEffectDrainConfig;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    entity_change::{EntityChange, EntityLifecycle},
    expanded::{
        attribute_change::{AttributeChange, AttributeChangeKind},
        instance_key::AttributeInstanceKey,
    },
    notification::ChangeOrigin,
};
use chrono::Utc;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use serial_test::serial;
use std::{sync::Arc, time::Duration};

/// The Entity the write modifies.
const ENTITY_ID: &str = "urn:ngsi-ld:Vehicle:change-round-trip";
/// Its expanded type.
const VEHICLE: &str = "https://uri.etsi.org/ngsi-ld/default-context/Vehicle";
/// The expanded Attribute the write updates.
const SPEED: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";
/// The expanded Attribute whose named instance the write deletes.
const NAME: &str = "https://uri.etsi.org/ngsi-ld/default-context/name";
/// The `datasetId` of the deleted `name` instance.
const DATASET: &str = "urn:ngsi-ld:Dataset:driver";

/// The expanded Entity the write leaves behind.
fn modified_entity() -> Value {
    json!({
        "@id": ENTITY_ID,
        "@type": [VEHICLE],
        SPEED: [{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 120}]
        }]
    })
}

/// The change: `speed` updated, the `driver` instance of `name` deleted.
fn modification() -> EntityChange {
    EntityChange::new(
        EntityLifecycle::Modified,
        vec![
            AttributeChange {
                key: AttributeInstanceKey {
                    attr: SPEED.parse().expect("a valid attribute name"),
                    dataset_id: None,
                },
                kind: AttributeChangeKind::Updated,
            },
            AttributeChange {
                key: AttributeInstanceKey {
                    attr: NAME.parse().expect("a valid attribute name"),
                    dataset_id: Some(DATASET.parse().expect("a valid dataset identifier")),
                },
                kind: AttributeChangeKind::Deleted,
            },
        ],
    )
}

#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_job_hands_each_write_its_whole_change_back() {
    let pool = queue_database().await;
    let event = ChangeEvent::builder()
        .entity_id(ENTITY_ID.parse().expect("a valid entity identifier"))
        .entity_types(vec![VEHICLE.parse().expect("a valid entity type")])
        .change(modification())
        .occurred_at(Utc::now())
        .origin(ChangeOrigin::Local)
        .current_payload(Some(Arc::new(modified_entity())))
        .build();
    let entity_id: Identifier = ENTITY_ID.parse().expect("a valid entity identifier");
    let write = AtomicWrite::builder()
        .entity_id(Arc::new(entity_id))
        .entity(EntityMutation::None)
        .side_effects(Some(BatchSideEffectJob {
            temporal_entities: Vec::new(),
            change_events: vec![event],
        }))
        .build();
    committer(&pool).commit(&live_target(), vec![write]).await.expect("the write commits its job");

    let payload: Value = sqlx::query_scalar("SELECT payload FROM pending_side_effects")
        .fetch_one(&pool)
        .await
        .expect("the queued job");
    assert_eq!(
        payload["change_events"].as_array().map(Vec::len),
        Some(1),
        "one write defers one change: {payload}"
    );
    assert_eq!(payload["change_events"][0]["lifecycle"], json!("modified"), "{payload}");
    assert_eq!(payload["change_events"][0]["current_payload"], modified_entity(), "{payload}");

    let (drain, published) = live_side_effect_drain(
        &pool,
        EntitySideEffectDrainConfig {
            lease_seconds: 30,
            batch_size: 4,
            max_attempts: 3,
            base_backoff: Duration::from_secs(1),
            lease_holder: side_effect_worker(),
        },
    );
    let outcome = drain.execute(Utc::now()).await.expect("the drain pass runs");
    assert_eq!(outcome.completed, 1, "the job is replayed and settled");

    let changes = published.take();
    assert_eq!(changes.len(), 1, "the one change is notified once");
    assert_eq!(changes[0].event.change, modification(), "the change comes back instance for instance");
    assert_eq!(
        changes[0].event.current_payload.as_deref(),
        Some(&modified_entity()),
        "the change carries the full Entity the write left behind"
    );
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM pending_side_effects")
        .fetch_one(&pool)
        .await
        .expect("remaining jobs");
    assert_eq!(remaining, 0, "the drained job left the queue");
    pool.close().await;
}
