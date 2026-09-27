//! A side-effect job stores the Entity its changes left behind once.
//!
//! One write creating an Entity raises `entityCreated` and `attributeCreated`
//! for it, and both changes carry the very Entity the write left behind: what
//! each notification renders and what its `q`, `geoQ` and `scopeQ` are
//! evaluated against (clause 5.8.6). The job queued for them stores that Entity
//! once -- the first change carries it as `{"stored": ...}`, the second refers
//! back to it as `{"as-event": 0}` -- and the drain hands both changes the full
//! Entity again, sharing one copy.
//!
//! The write is committed with the broker's own committer, carrying two
//! changes that share one Entity allocation exactly as the entity service
//! raises them, and the job is drained by the real side-effect drain in live
//! delivery, whose published changes are what its notification worker renders.

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
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::notification::{ChangeOrigin, NotificationTrigger};
use chrono::Utc;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use serial_test::serial;
use std::{sync::Arc, time::Duration};

/// The Entity the write creates.
const ENTITY_ID: &str = "urn:ngsi-ld:Vehicle:shared-document";
/// Its expanded type.
const VEHICLE: &str = "https://uri.etsi.org/ngsi-ld/default-context/Vehicle";
/// The expanded Attribute it is created with.
const SPEED: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";

/// The expanded Entity the write leaves behind.
fn created_entity() -> Value {
    json!({
        "@id": ENTITY_ID,
        "@type": [VEHICLE],
        SPEED: [{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 120}]
        }]
    })
}

/// One change the write raises, carrying the Entity it left behind.
fn change(trigger: NotificationTrigger, changed_attributes: &[&str], entity: &Arc<Value>) -> ChangeEvent {
    ChangeEvent::builder()
        .entity_id(ENTITY_ID.parse().expect("a valid entity identifier"))
        .entity_types(vec![VEHICLE.parse().expect("a valid entity type")])
        .trigger_reason(trigger)
        .changed_attributes(
            changed_attributes
                .iter()
                .map(|attribute| attribute.parse().expect("a valid attribute name"))
                .collect(),
        )
        .occurred_at(Utc::now())
        .origin(ChangeOrigin::Local)
        .current_payload(Some(Arc::clone(entity)))
        .build()
}

#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_job_stores_the_entity_its_changes_share_once_and_the_drain_hands_it_to_both() {
    let pool = queue_database().await;
    let entity = Arc::new(created_entity());
    let job = BatchSideEffectJob {
        temporal_entities: Vec::new(),
        change_events: vec![
            change(NotificationTrigger::EntityCreated, &[], &entity),
            change(NotificationTrigger::AttributeCreated, &[SPEED], &entity),
        ],
    };
    let entity_id: Uri = ENTITY_ID.parse().expect("a valid entity identifier");
    let write = AtomicWrite::builder()
        .entity_id(Arc::new(entity_id))
        .entity(EntityMutation::None)
        .side_effects(Some(job))
        .build();
    committer(&pool).commit(&live_target(), vec![write]).await.expect("the write commits its job");

    let payload: Value = sqlx::query_scalar("SELECT payload FROM pending_side_effects")
        .fetch_one(&pool)
        .await
        .expect("the queued job");
    let stored: Vec<&Value> = payload["change_events"]
        .as_array()
        .expect("the job's change events")
        .iter()
        .map(|change| &change["current_payload"])
        .collect();
    assert_eq!(
        stored,
        vec![&json!({"stored": created_entity()}), &json!({"as-event": 0})],
        "the first change stores the Entity and the second refers back to it: {payload}"
    );

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
    let triggers: Vec<NotificationTrigger> = changes.iter().map(|published| published.event.trigger_reason).collect();
    assert_eq!(
        triggers,
        vec![NotificationTrigger::EntityCreated, NotificationTrigger::AttributeCreated],
        "both changes are notified"
    );
    let documents: Vec<&Arc<Value>> = changes
        .iter()
        .map(|published| published.event.current_payload.as_ref().expect("a change carrying the Entity it left behind"))
        .collect();
    for document in &documents {
        assert_eq!(document.as_ref(), &created_entity(), "each change carries the full Entity");
    }
    let first = documents.first().expect("the first change's Entity");
    assert!(
        documents.iter().all(|document| Arc::ptr_eq(first, document)),
        "the drained changes share one copy of the Entity again"
    );
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM pending_side_effects")
        .fetch_one(&pool)
        .await
        .expect("remaining jobs");
    assert_eq!(remaining, 0, "the drained job left the queue");
    pool.close().await;
}
