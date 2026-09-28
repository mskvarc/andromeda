//! Brokers in different notification delivery modes sharing one tenant
//! database: a change a live-delivery broker defers to a `Prefer:
//! respond-async` batch's side-effect job is notified exactly once (clause
//! 5.8.6) when a durable-delivery broker drains that job.
//!
//! The stack runs `broker` and `broker-replica` under durable delivery and
//! `broker-wide` and `broker-direct` under live delivery. A live-delivery
//! broker defers a batch's changes to its side-effect job, and any broker
//! sharing the tenant database may drain it; a durable-delivery broker that
//! does queues the deliveries the changes owe rather than publishing them to an
//! in-process channel its notification worker never reads.
//!
//! The mirror case -- a durable-delivery broker's job drained by a live-delivery
//! broker -- is not driven here: a broker's own commit wakes its own drain, so
//! no request can hand the job to another broker, and whichever broker drains
//! it the job carries no change, because the durable broker recorded the match
//! with the write. That producer invariant is guarded by
//! `batch_create_records_the_match_and_defers_no_event_when_enqueued` in
//! `andromeda-entity`.

use crate::{client, control_pool, reset};
use andromeda_context_commit::{write_ordinal::WriteOrdinal, write_record::WriteRecord};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_ngsi_ld_model::{entity_change::EntityLifecycle, expanded::attribute_change::AttributeChangeKind, notification::ChangeOrigin};
use andromeda_postgresql_context_store::pending_side_effects::{
    payload::{PersistedAttributeChange, PersistedBatchSideEffectJob, PersistedChangeEvent},
    repo::PendingSideEffectsRepository,
};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::pooler_probe::{endpoints, tenant_database::connect_tenant_database, tenant_load};
use andromeda_test_mocks::{notification_server::NotificationServer, server_config::MockServerConfig};
use chrono::Utc;
use reqwest::Client;
use serde_json::{Value, json};
use std::{collections::HashMap, slice, time::Duration};
use tokio::time::Instant;

/// The expanded `Device` entity type the subscription watches.
const DEVICE: &str = "https://uri.etsi.org/ngsi-ld/default-context/Device";
/// The expanded attribute the deferred creation names.
const TEMPERATURE: &str = "https://uri.etsi.org/ngsi-ld/default-context/temperature";
/// How long the first notification is waited for. The drain is woken by the
/// commit of the batch that follows the queued job, so this is a bound on a
/// wake, a drain pass, a durable delivery and one HTTP round trip, with a wide
/// margin; it is far below the fallback the drain would otherwise rest on.
const FIRST_NOTIFICATION_DEADLINE: Duration = Duration::from_secs(60);
/// How long the notification stream must stay quiet after its first
/// notification before it is taken as settled.
const QUIET: Duration = Duration::from_secs(8);
/// The longest the notifications that follow the first are collected for.
const SETTLE_DEADLINE: Duration = Duration::from_secs(60);

/// Subscribe `callback` to the creation of every `Device` under `tenant`.
///
/// Only `entityCreated` is watched, so each created entity is owed exactly one
/// notification (clause 5.8.6). The subscribing broker's matcher holds the
/// subscription before the creation is acknowledged.
async fn subscribe_creations(client: &Client, broker: &str, tenant: &str, callback: &str) {
    let response = client
        .post(format!("{broker}subscriptions"))
        .header("NGSILD-Tenant", tenant)
        .header("Content-Type", "application/ld+json")
        .json(&json!({
            "type": "Subscription",
            "entities": [{"type": "Device"}],
            "notificationTrigger": ["entityCreated"],
            "notification": {"endpoint": {"uri": callback, "accept": "application/json"}},
            "@context": [CORE_CONTEXT]
        }))
        .send()
        .await
        .expect("the subscription request is answered");
    assert_eq!(response.status().as_u16(), 201, "the subscription is created");
}

/// Commit a `Prefer: respond-async` batch creating one entity no subscription
/// watches on `broker`.
///
/// The batch defers the entity's observation to a side-effect job of its own,
/// and the commit that queues it wakes the broker's side-effect drain, which
/// then drains every job the tenant database holds.
async fn wake_side_effect_drain(client: &Client, broker: &str, tenant: &str) {
    let marker = json!({
        "id": format!("urn:ngsi-ld:DrainWake:{tenant}"),
        "type": "DrainWake",
        "reading": {"type": "Property", "value": 1},
        "@context": [CORE_CONTEXT]
    });
    let response = client
        .post(format!("{broker}entityOperations/create"))
        .header("NGSILD-Tenant", tenant)
        .header("Content-Type", "application/ld+json")
        .header("Prefer", "respond-async")
        .json(&[marker])
        .send()
        .await
        .expect("the waking batch is answered");
    assert!(response.status().is_success(), "the waking batch lands: {}", response.status());
}

/// Count the notified entity identifiers: the first notification is awaited
/// for [`FIRST_NOTIFICATION_DEADLINE`], and the rest until the stream stays
/// quiet for [`QUIET`] or [`SETTLE_DEADLINE`] passes.
async fn notified_entities(server: &NotificationServer) -> HashMap<String, usize> {
    let mut notified: HashMap<String, usize> = HashMap::new();
    let first = server
        .next_request(FIRST_NOTIFICATION_DEADLINE)
        .await
        .expect("the deferred change is notified before the deadline");
    count_entities(&mut notified, &first.json().expect("a notification body is JSON"));
    let deadline = Instant::now() + SETTLE_DEADLINE;
    while Instant::now() < deadline {
        let Ok(request) = server.next_request(QUIET).await else {
            break;
        };
        count_entities(&mut notified, &request.json().expect("a notification body is JSON"));
    }
    notified
}

/// Add one to the count of every entity `notification` carries.
fn count_entities(notified: &mut HashMap<String, usize>, notification: &Value) {
    for id in notification["data"].as_array().into_iter().flatten().filter_map(|entity| entity["id"].as_str()) {
        *notified.entry(id.to_owned()).or_default() += 1;
    }
}

/// The job exactly as a live-delivery broker defers the creation of `entity_id`:
/// no observation left to record and one change, carrying the entity the write
/// left behind, still owed its notification.
fn live_deferred_creation(entity_id: &str) -> PersistedBatchSideEffectJob {
    let change = PersistedChangeEvent {
        entity_id: entity_id.parse().expect("a valid entity identifier"),
        entity_types: vec![DEVICE.parse().expect("a valid entity type")],
        lifecycle: EntityLifecycle::Created,
        instances: vec![PersistedAttributeChange {
            attribute: TEMPERATURE.parse().expect("a valid attribute name"),
            dataset_id: None,
            kind: AttributeChangeKind::Created,
        }],
        occurred_at: Utc::now(),
        entity_snapshot: None,
        previous_attributes: None,
        sys_attrs_snapshot: None,
        origin: ChangeOrigin::Local,
        current_payload: Some(json!({
            "@id": entity_id,
            "@type": [DEVICE],
            TEMPERATURE: [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 21}]
            }]
        })),
    };
    PersistedBatchSideEffectJob {
        temporal_entities: Vec::new(),
        change_events: vec![change],
    }
}

/// A live-delivery broker defers the changes of a respond-async batch to its
/// side-effect job. Queued in a tenant only a durable-delivery broker holds
/// resident, the job must still notify its change -- once.
///
/// A durable-delivery broker that published a drained job's changes to its
/// in-process channel, instead of recording the matches they owe, would never
/// notify the change: its notification worker does not read that channel, so
/// the first notification would never arrive.
#[tokio::test]
#[serial_test::serial]
async fn a_live_brokers_deferred_change_is_notified_once_when_a_durable_broker_drains_it() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let durable = endpoints::broker();
    let server = NotificationServer::start(MockServerConfig::from_environment().expect("mock server config"))
        .await
        .expect("notification server");
    let callback = server.endpoint_url().expect("callback url");

    let tenant = tenant_load::tenant_names("pgbmixlive", 1).remove(0);
    tenant_load::provision_tenants(&client, &durable, slice::from_ref(&tenant), CORE_CONTEXT, 1)
        .await
        .expect("the tenant is provisioned");
    subscribe_creations(&client, &durable, &tenant, &callback).await;

    // The tenant's provisioning entity exists, so nothing about the delivery
    // depends on the job alone. The job is queued behind the broker's back,
    // exactly as a live-delivery broker's commit leaves it; queuing it through
    // such a broker would wake that broker's own drain instead.
    let entity_id = format!("urn:ngsi-ld:Device:provision:{tenant}");
    let tenant_name: TenantName = tenant.parse().expect("a provisioned tenant has a valid name");
    let tenant_database = connect_tenant_database(&pool, &tenant_name).await.expect("the tenant database is reachable");
    let mut transaction = tenant_database.begin().await.expect("the enqueue transaction begins");
    PendingSideEffectsRepository::enqueue_in_tx(
        &mut transaction,
        &WriteRecord::mint(Utc::now()),
        WriteOrdinal::at(0),
        &live_deferred_creation(&entity_id),
    )
    .await
    .expect("the deferred job is queued");
    transaction.commit().await.expect("the deferred job commits");
    tenant_database.close().await;
    wake_side_effect_drain(&client, &durable, &tenant).await;

    let notified = notified_entities(&server).await;
    server.shutdown().await.expect("shutdown");
    assert_eq!(
        notified.get(&entity_id).copied(),
        Some(1),
        "the deferred creation is notified exactly once: {notified:?}"
    );
    assert_eq!(notified.len(), 1, "only the deferred creation is notified: {notified:?}");
    reset(&client, &pool).await;
    pool.close().await;
}
