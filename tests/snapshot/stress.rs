//! Bounded Snapshot stress acceptance cases.

use crate::{
    notification_adapter::NotificationAdapter,
    support::{broker_url, clean_database, client, clone_snapshot_raw, create_snapshot_raw, get_snapshot, wait_for_snapshot_terminal},
};
use andromeda_ngsi_ld_lexicon::time::rendering::instant_text;
use andromeda_test_harness::database_reset::reset_owned_tenant;
use chrono::{TimeZone, Utc};
use futures::{StreamExt, future::join_all, stream};
use reqwest::StatusCode;
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    time::Duration,
};
use tokio::{
    task::yield_now,
    time::{MissedTickBehavior, interval, sleep, timeout},
};

const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";
const REMOTE_BASE: &str = "http://127.0.0.1:1083/ngsi-ld/v1";
const HEAVY_DEADLINE: Duration = Duration::from_secs(300);

async fn reset_remote_database() {
    let primary_url = env::var("ANDROMEDA_TEST_DATABASE_URL").expect("Snapshot database URL");
    let (server_url, _) = primary_url.rsplit_once('/').expect("database URL has a database path");
    let remote_url = server_url.replace("127.0.0.1:5481", "127.0.0.1:5482");
    let remote_pool = PgPool::connect(&format!("{remote_url}/snapshot_remote"))
        .await
        .expect("remote Snapshot database connection");
    reset_owned_tenant(&remote_pool).await.expect("remote Snapshot database reset");
}

/// A current entity carrying one Property. The attribute is not named `value`:
/// that term is the core @context's `ngsi-ld:hasValue`, which clause 4.5.1 keeps
/// from naming an attribute.
fn current_entity(id: &str, entity_type: &str, value: usize) -> Value {
    json!({"id": id, "type": entity_type, "reading": {"type": "Property", "value": value}, "@context": CORE_CONTEXT})
}

fn temporal_entity(id: &str, entity_type: &str, offset: usize) -> Value {
    let start = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).single().expect("static timestamp");
    let observations = (0..20)
        .map(|index| {
            let observed_at = instant_text(
                start
                    .checked_add_signed(chrono::Duration::seconds(i64::try_from(offset * 20 + index).expect("stress index range")))
                    .expect("stress timestamp range"),
            );
            json!({"type": "Property", "value": index, "observedAt": observed_at})
        })
        .collect::<Vec<_>>();
    json!({"id": id, "type": entity_type, "speed": observations, "@context": CORE_CONTEXT})
}

async fn create_current_batch_at(base: &str, entities: &[Value]) {
    for chunk in entities.chunks(256) {
        let response = client()
            .post(format!("{base}/entityOperations/create"))
            .header("Content-Type", "application/ld+json")
            .json(chunk)
            .send()
            .await
            .expect("stress current batch create");
        assert!(
            [StatusCode::CREATED, StatusCode::MULTI_STATUS].contains(&response.status()),
            "current batch status: {}",
            response.status()
        );
    }
}

async fn create_temporal_set_at(base: &str, entities: Vec<Value>) {
    let statuses = stream::iter(entities)
        .map(|body| async move {
            client()
                .post(format!("{base}/temporal/entities"))
                .header("Content-Type", "application/ld+json")
                .json(&body)
                .send()
                .await
                .expect("stress temporal create")
                .status()
        })
        .buffer_unordered(24)
        .collect::<Vec<_>>()
        .await;
    assert!(statuses.into_iter().all(|status| status == StatusCode::CREATED));
}

async fn register_remote(entity_type: &str, suffix: &str, operations: &[&str]) {
    let registration = json!({
        "id": format!("urn:ngsi-ld:ContextSourceRegistration:{suffix}"),
        "type": "ContextSourceRegistration",
        "endpoint": "http://snapshot-remote:1083",
        "information": [{"entities": [{"type": entity_type}]}],
        "operations": operations,
        "@context": CORE_CONTEXT
    });
    pretty_assertions::assert_eq!(
        client()
            .post(broker_url("/csourceRegistrations"))
            .header("Content-Type", "application/ld+json")
            .json(&registration)
            .send()
            .await
            .expect("stress remote registration")
            .status(),
        StatusCode::CREATED
    );
}

async fn wait_terminal_for_tenant(tenant: &str, id: &str) -> Value {
    timeout(Duration::from_secs(15), async {
        loop {
            let response = client()
                .get(broker_url(&format!("/snapshots/{id}")))
                .header("NGSILD-Tenant", tenant)
                .send()
                .await
                .expect("tenant Snapshot poll");
            pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
            let body: Value = response.json().await.expect("tenant Snapshot body");
            if body["snapshotStatus"] != "preparing" {
                return body;
            }
            yield_now().await;
        }
    })
    .await
    .expect("tenant Snapshot terminal")
}

async fn wait_terminal_for_stress(id: &str) -> Value {
    timeout(Duration::from_secs(240), async {
        let mut polls = interval(Duration::from_millis(250));
        polls.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            polls.tick().await;
            let (status, snapshot) = get_snapshot(&client(), id).await;
            pretty_assertions::assert_eq!(status, StatusCode::OK);
            if snapshot["snapshotStatus"] != "preparing" {
                return snapshot;
            }
        }
    })
    .await
    .expect("stress Snapshot reaches a terminal state within four minutes")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[serial(snapshot)]
async fn snapshot_stress_capture_4000_distributed_entities_and_64_scoped_clients() {
    timeout(HEAVY_DEADLINE, async {
        let _serialized_environment = clean_database().await;
        reset_remote_database().await;
        let entity_type = "https://example.org/StressDistributedVehicle";
        let local = (0..2_000)
            .map(|index| current_entity(&format!("urn:ngsi-ld:StressDistributedVehicle:local-{index:04}"), entity_type, index))
            .collect::<Vec<_>>();
        let remote = (0..2_000)
            .map(|index| current_entity(&format!("urn:ngsi-ld:StressDistributedVehicle:remote-{index:04}"), entity_type, index))
            .collect::<Vec<_>>();
        create_current_batch_at(&broker_url(""), &local).await;
        create_current_batch_at(REMOTE_BASE, &remote).await;
        register_remote(entity_type, "stress-current", &["queryEntity"]).await;
        let snapshot = "urn:ngsi-ld:Snapshot:stress-current";
        pretty_assertions::assert_eq!(
            create_snapshot_raw(
                &client(),
                &json!({"id": snapshot, "type": "Snapshot", "snapshotQueries": [{"type": "Query", "entities": [{"type": entity_type}]}]})
            )
            .await
            .0,
            StatusCode::CREATED
        );
        pretty_assertions::assert_eq!(wait_terminal_for_stress(snapshot).await["snapshotStatus"], "success");
        let count = client()
            .get(broker_url(&format!("/entities?type={}&limit=1&count=true", urlencoding::encode(entity_type))))
            .header("NGSILD-Snapshot", snapshot)
            .send()
            .await
            .expect("captured entity count");
        pretty_assertions::assert_eq!(count.status(), StatusCode::OK);
        pretty_assertions::assert_eq!(count.headers()["NGSILD-Results-Count"], "4000");
        let clients = (0..64).map(|index| async move {
            let origin = if index % 2 == 0 { "local" } else { "remote" };
            let id = format!("urn:ngsi-ld:StressDistributedVehicle:{origin}-{:04}", index % 2_000);
            client()
                .get(broker_url(&format!("/entities/{id}")))
                .header("NGSILD-Snapshot", snapshot)
                .send()
                .await
                .expect("concurrent captured entity read")
                .status()
        });
        assert!(join_all(clients).await.into_iter().all(|status| status == StatusCode::OK));
    })
    .await
    .expect("4,000-entity Snapshot stress case finishes within five minutes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[serial(snapshot)]
async fn snapshot_stress_capture_20000_temporal_observations_and_64_clients() {
    timeout(HEAVY_DEADLINE, async {
        let _serialized_environment = clean_database().await;
        reset_remote_database().await;
        let entity_type = "https://example.org/StressTemporalVehicle";
        let local = (0..500)
            .map(|index| temporal_entity(&format!("urn:ngsi-ld:StressTemporalVehicle:local-{index:04}"), entity_type, index))
            .collect::<Vec<_>>();
        let remote = (0..500)
            .map(|index| temporal_entity(&format!("urn:ngsi-ld:StressTemporalVehicle:remote-{index:04}"), entity_type, index + 500))
            .collect::<Vec<_>>();
        create_temporal_set_at(&broker_url(""), local).await;
        create_temporal_set_at(REMOTE_BASE, remote).await;
        register_remote(entity_type, "stress-temporal", &["queryTemporal"]).await;
        let snapshot = "urn:ngsi-ld:Snapshot:stress-temporal";
        let query = json!({"type": "Query", "entities": [{"type": entity_type}], "temporalQ": {"timerel": "after", "timeAt": "2024-01-01T00:00:00Z"}});
        pretty_assertions::assert_eq!(
            create_snapshot_raw(&client(), &json!({"id": snapshot, "type": "Snapshot", "snapshotTemporalQueries": [query]}))
                .await
                .0,
            StatusCode::CREATED
        );
        pretty_assertions::assert_eq!(wait_terminal_for_stress(snapshot).await["snapshotStatus"], "success");
        let pool = PgPool::connect(&env::var("ANDROMEDA_TEST_DATABASE_URL").expect("Snapshot database URL"))
            .await
            .expect("Snapshot database connection");
        let snapshot_key: i64 = sqlx::query_scalar("SELECT snapshot_key FROM snapshots WHERE id = $1")
            .bind(snapshot)
            .fetch_one(&pool)
            .await
            .expect("captured snapshot key");
        // Each published Snapshot branch owns its history in schema `snap_<key>`.
        let observations: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM snap_{snapshot_key}.attribute_history")))
            .fetch_one(&pool)
            .await
            .expect("captured observation count");
        pretty_assertions::assert_eq!(observations, 20_000);
        let clients = (0..64).map(|index| async move {
            let origin = if index % 2 == 0 { "local" } else { "remote" };
            let id = format!("urn:ngsi-ld:StressTemporalVehicle:{origin}-{:04}", index % 500);
            client()
                .get(broker_url(&format!(
                    "/temporal/entities/{id}?timerel=after&timeAt=2024-01-01T00%3A00%3A00Z&attrs=speed&lastN=20"
                )))
                .header("NGSILD-Snapshot", snapshot)
                .send()
                .await
                .expect("concurrent captured temporal read")
                .status()
        });
        assert!(join_all(clients).await.into_iter().all(|status| status == StatusCode::OK));
    })
    .await
    .expect("20,000-observation Snapshot stress case finishes within five minutes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[serial(snapshot)]
async fn snapshot_stress_clone_large_mixed_branch_is_complete_and_independent() {
    timeout(HEAVY_DEADLINE, async {
        let _serialized_environment = clean_database().await;
        let source = "urn:ngsi-ld:Snapshot:stress-clone-source";
        let target = "urn:ngsi-ld:Snapshot:stress-clone-target";
        pretty_assertions::assert_eq!(
            create_snapshot_raw(&client(), &json!({"id": source, "type": "Snapshot", "snapshotQueries": []}))
                .await
                .0,
            StatusCode::CREATED
        );
        wait_for_snapshot_terminal(&client(), source, Duration::from_secs(10)).await;
        let current_type = "https://example.org/StressCloneCurrent";
        let current = (0..4_000)
            .map(|index| current_entity(&format!("urn:ngsi-ld:StressCloneCurrent:{index:04}"), current_type, index))
            .collect::<Vec<_>>();
        for chunk in current.chunks(256) {
            let response = client()
                .post(broker_url("/entityOperations/create"))
                .header("Content-Type", "application/ld+json")
                .header("NGSILD-Snapshot", source)
                .json(chunk)
                .send()
                .await
                .expect("source branch current batch");
            assert!([StatusCode::CREATED, StatusCode::MULTI_STATUS].contains(&response.status()));
        }
        let temporal_type = "https://example.org/StressCloneTemporal";
        let temporal = (0..1_000)
            .map(|index| temporal_entity(&format!("urn:ngsi-ld:StressCloneTemporal:{index:04}"), temporal_type, index))
            .collect::<Vec<_>>();
        let temporal_statuses = stream::iter(temporal)
            .map(|body| async move {
                client()
                    .post(broker_url("/temporal/entities"))
                    .header("Content-Type", "application/ld+json")
                    .header("NGSILD-Snapshot", source)
                    .json(&body)
                    .send()
                    .await
                    .expect("source branch temporal create")
                    .status()
            })
            .buffer_unordered(24)
            .collect::<Vec<_>>()
            .await;
        assert!(temporal_statuses.into_iter().all(|status| status == StatusCode::CREATED));
        pretty_assertions::assert_eq!(
            clone_snapshot_raw(&client(), source, &json!({"id": target, "type": "Snapshot"})).await.0,
            StatusCode::CREATED
        );
        let readers = (0..64).map(|index| async move {
            client()
                .get(broker_url(&format!("/entities/urn:ngsi-ld:StressCloneCurrent:{index:04}")))
                .header("NGSILD-Snapshot", source)
                .send()
                .await
                .expect("source read during clone")
                .status()
        });
        assert!(join_all(readers).await.into_iter().all(|status| status == StatusCode::OK));
        pretty_assertions::assert_eq!(wait_terminal_for_stress(target).await["snapshotStatus"], "success");
        let pool = PgPool::connect(&env::var("ANDROMEDA_TEST_DATABASE_URL").expect("Snapshot database URL"))
            .await
            .expect("Snapshot database connection");
        // Each Snapshot branch owns its rows in its own schema `snap_<key>`, so the
        // clone's completeness is read from the source and target branch schemas
        // directly rather than through a shared partition column.
        let mut counts: Vec<(String, i64, i64)> = Vec::new();
        for id in [source, target] {
            let key: i64 = sqlx::query_scalar("SELECT snapshot_key FROM snapshots WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .expect("mixed branch snapshot key");
            let current_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM snap_{key}.entities")))
                .fetch_one(&pool)
                .await
                .expect("mixed branch entity count");
            let temporal_count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM snap_{key}.attribute_history")))
                .fetch_one(&pool)
                .await
                .expect("mixed branch history count");
            counts.push((id.to_string(), current_count, temporal_count));
        }
        pretty_assertions::assert_eq!(counts.len(), 2);
        for (_, current_count, temporal_count) in counts {
            // Integrated Core creates contribute one temporal side effect each,
            // while Temporal creates also materialize their current state.
            pretty_assertions::assert_eq!((current_count, temporal_count), (5_000, 24_000));
        }
        pretty_assertions::assert_eq!(
            client()
                .delete(broker_url("/entities/urn:ngsi-ld:StressCloneCurrent:0000"))
                .header("NGSILD-Snapshot", source)
                .send()
                .await
                .expect("source-only post-clone delete")
                .status(),
            StatusCode::NO_CONTENT
        );
        pretty_assertions::assert_eq!(
            client()
                .get(broker_url("/entities/urn:ngsi-ld:StressCloneCurrent:0000"))
                .header("NGSILD-Snapshot", target)
                .send()
                .await
                .expect("clone independence read")
                .status(),
            StatusCode::OK
        );
    })
    .await
    .expect("large mixed clone stress case finishes within five minutes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[serial(snapshot)]
async fn snapshot_stress_64_branch_subscriptions_deliver_and_cleanup() {
    timeout(HEAVY_DEADLINE, async {
        let _serialized_environment = clean_database().await;
        let receiver = NotificationAdapter::start().await;
        let entity_type = "https://example.org/StressSubscriptionVehicle";
        let mut expected = BTreeMap::new();
        let tenant_suffix = uuid::Uuid::new_v4().simple().to_string();
        let tenants = [format!("stress-tenant-a-{tenant_suffix}"), format!("stress-tenant-b-{tenant_suffix}")];
        for tenant in &tenants {
            let provision = current_entity(&format!("urn:ngsi-ld:StressSubscriptionTenant:{tenant}"), entity_type, 0);
            pretty_assertions::assert_eq!(
                client()
                    .post(broker_url("/entities"))
                    .header("Content-Type", "application/ld+json")
                    .header("NGSILD-Tenant", tenant)
                    .json(&provision)
                    .send()
                    .await
                    .expect("stress tenant provisioning Entity")
                    .status(),
                StatusCode::CREATED
            );
            for branch_index in 0..4 {
                let snapshot = format!("urn:ngsi-ld:Snapshot:stress-sub-{tenant}-{branch_index}");
                let create = client()
                    .post(broker_url("/snapshots"))
                    .header("Content-Type", "application/json")
                    .header("NGSILD-Tenant", tenant)
                    .json(&json!({"id": snapshot, "type": "Snapshot", "snapshotQueries": []}))
                    .send()
                    .await
                    .expect("stress tenant Snapshot create");
                pretty_assertions::assert_eq!(create.status(), StatusCode::CREATED);
                wait_terminal_for_tenant(tenant, &snapshot).await;
                for subscription_index in 0..8 {
                    let subscription_id = format!("urn:ngsi-ld:Subscription:stress-{tenant}-{branch_index}-{subscription_index}");
                    let body = json!({
                        "id": subscription_id,
                        "type": "Subscription",
                        "entities": [{"type": entity_type}],
                        "notification": {"format": "normalized", "endpoint": {"uri": receiver.endpoint_url(), "accept": "application/json"}},
                        "@context": CORE_CONTEXT
                    });
                    let response = client()
                        .post(broker_url("/subscriptions"))
                        .header("Content-Type", "application/ld+json")
                        .header("NGSILD-Tenant", tenant)
                        .header("NGSILD-Snapshot", &snapshot)
                        .json(&body)
                        .send()
                        .await
                        .expect("stress scoped subscription create");
                    pretty_assertions::assert_eq!(response.status(), StatusCode::CREATED);
                    expected.insert(subscription_id, (tenant.clone(), snapshot.clone()));
                }
            }
        }
        sleep(Duration::from_millis(500)).await;
        receiver.clear_notifications().await;
        let mutations = expected
            .values()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .enumerate()
            .map(|(index, (tenant, snapshot))| async move {
                let body = current_entity(&format!("urn:ngsi-ld:StressSubscriptionVehicle:{tenant}-{index}"), entity_type, index);
                client()
                    .post(broker_url("/entities"))
                    .header("Content-Type", "application/ld+json")
                    .header("NGSILD-Tenant", tenant)
                    .header("NGSILD-Snapshot", snapshot)
                    .json(&body)
                    .send()
                    .await
                    .expect("stress branch mutation")
                    .status()
            });
        assert!(join_all(mutations).await.into_iter().all(|status| status == StatusCode::CREATED));
        let mut received = BTreeSet::new();
        for _ in 0..64 {
            let notification = receiver.wait_for_notification(Duration::from_secs(30)).await;
            let subscription_id = notification.body["subscriptionId"].as_str().expect("subscription id").to_owned();
            let (tenant, snapshot) = expected.get(&subscription_id).expect("expected subscription notification");
            pretty_assertions::assert_eq!(notification.headers["NGSILD-Tenant"], tenant);
            pretty_assertions::assert_eq!(notification.headers["NGSILD-Snapshot"], snapshot);
            assert!(received.insert(subscription_id), "each subscription delivers exactly once");
        }
        pretty_assertions::assert_eq!(received.len(), 64);
        for (subscription_id, (tenant, snapshot)) in expected {
            pretty_assertions::assert_eq!(
                client()
                    .delete(broker_url(&format!("/subscriptions/{subscription_id}")))
                    .header("NGSILD-Tenant", &tenant)
                    .header("NGSILD-Snapshot", &snapshot)
                    .send()
                    .await
                    .expect("stress subscription delete")
                    .status(),
                StatusCode::NO_CONTENT
            );
        }
        receiver.shutdown().await;
    })
    .await
    .expect("64-subscription Snapshot stress case finishes within five minutes");
}
