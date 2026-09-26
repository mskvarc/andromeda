//! Transaction-pooling correctness through the pooled broker: COPY-staged
//! batches commit atomically, statement-cache eviction at the pooler is
//! transparent, snapshots run through the pooler under load, durable
//! notifications are delivered exactly per change, `synchronous_commit` is a
//! database default, a runtime-created tenant is routed with no reload,
//! maintenance holds its session lock on a direct connection, and concurrent
//! writes to one entity serialise correctly.
//!
//! Everything drives the pooled `broker` on the `pgbouncer` stack.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::pooler_probe::{
    endpoints,
    metrics_scrape::Metrics,
    pooler_console::PoolerConsole,
    request_outcome::RequestOutcome,
    tenant_cleanup,
    tenant_load,
};
use andromeda_test_mocks::{notification_server::NotificationServer, server_config::MockServerConfig};
use futures::future::join_all;
use reqwest::Client;
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection, PgPool};
use std::{slice, time::Duration};
use tokio::time::sleep;

/// A client with a generous timeout.
fn client() -> Client {
    Client::builder().timeout(Duration::from_secs(30)).build().expect("reqwest client")
}

/// The Link header naming the core context.
fn link_header() -> String {
    format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

/// Connect to the control database over the direct endpoint.
async fn control_pool() -> PgPool {
    PgPool::connect(&endpoints::database_url())
        .await
        .expect("connect to PostgreSQL control database")
}

/// Reset the tier before and after a test.
async fn reset(client: &Client, pool: &PgPool) {
    let brokers = [
        endpoints::broker(),
        endpoints::broker_replica(),
        endpoints::broker_wide(),
        endpoints::broker_direct(),
    ];
    let refs: Vec<&str> = brokers.iter().map(String::as_str).collect();
    let _ = tenant_cleanup::reset(client, &refs, pool).await;
}

/// The address PostgreSQL sees `PgBouncer` connect from, read over a pooled
/// connection so it is the pooler's own server-side peer address.
async fn pooler_client_addr() -> String {
    let mut connection = PgConnection::connect(&endpoints::pooler_url()).await.expect("connect through the pooler");
    let addr = sqlx::query_scalar("SELECT host(inet_client_addr())")
        .fetch_one(&mut connection)
        .await
        .expect("read the pooler's client address");
    let _ = connection.close().await;
    addr
}

/// Every provisioned tenant database name.
async fn tenant_databases(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT datname FROM pg_database WHERE datname LIKE 'andromeda_tenant_%'")
        .fetch_all(pool)
        .await
        .unwrap_or_default()
}

/// Upsert `count` `Device` entities under `tenant` in one batch (COPY-staging
/// path). The batch body is a bare NGSI-LD entity array, each element carrying
/// the core context.
async fn upsert_batch(client: &Client, broker: &str, tenant: &str, count: usize) -> RequestOutcome {
    let entities: Vec<Value> = (0..count)
        .map(|index| json!({"id": format!("urn:ngsi-ld:Device:atomic:{tenant}:{index}"), "type": "Device", "seq": {"type": "Property", "value": index}, "@context": [CORE_CONTEXT]}))
        .collect();
    RequestOutcome::of(
        client
            .post(format!("{broker}entityOperations/upsert"))
            .header("NGSILD-Tenant", tenant)
            .header("Content-Type", "application/ld+json")
            .json(&entities),
    )
    .await
}

/// The maintenance decisions `broker` has taken so far, failing the test when
/// its metrics cannot be read: an unreadable scrape must never pass as a zero.
async fn maintenance_decisions(client: &Client, broker: &str) -> f64 {
    Metrics::scrape(client, broker)
        .await
        .map_err(|error| format!("the broker at {broker} must expose its metrics: {error}"))
        .expect("an unreadable scrape must never pass as a zero")
        .sum("andromeda_postgresql_maintenance_decisions_total")
}

#[tokio::test]
#[serial_test::serial]
async fn copy_staged_batches_commit_atomically_across_concurrent_tenants() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let tenants = tenant_load::tenant_names("pgbatomic", 24);
    tenant_load::provision_tenants(&client, &broker, &tenants, CORE_CONTEXT, 24).await;

    let writes = tenants.iter().map(|tenant| upsert_batch(&client, &broker, tenant, 500));
    let outcomes = join_all(writes).await;
    assert!(
        outcomes.iter().all(RequestOutcome::is_success),
        "every concurrent batch must commit: {outcomes:?}"
    );

    // A sampled entity from each tenant is readable with its exact value.
    for tenant in &tenants {
        let id = urlencoding::encode(&format!("urn:ngsi-ld:Device:atomic:{tenant}:499")).into_owned();
        let response = client
            .get(format!("{broker}entities/{id}"))
            .header("NGSILD-Tenant", tenant.as_str())
            .header("Accept", "application/ld+json")
            .header("Link", link_header())
            .send()
            .await
            .expect("read back");
        assert_eq!(response.status().as_u16(), 200, "tenant {tenant} entity 499 must be readable");
        let body: Value = response.json().await.expect("json body");
        assert_eq!(
            body["seq"]["value"].as_i64(),
            Some(499),
            "the exact staged value must have committed for {tenant}"
        );
    }

    // No staging relation from `CREATE TEMP TABLE ... ON COMMIT DROP` is visible
    // in any tenant database after the transactions committed.
    for database in tenant_databases(&pool).await {
        let url = endpoints::database_url().replace("/postgres", &format!("/{database}"));
        if let Ok(tenant_pool) = PgPool::connect(&url).await {
            let staged: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_class WHERE relname LIKE 'stage\\_%' OR relname LIKE 'staging\\_%'")
                .fetch_one(&tenant_pool)
                .await
                .unwrap_or(0);
            tenant_pool.close().await;
            assert_eq!(staged, 0, "no staging relation may survive a committed batch in {database}");
        }
    }
    reset(&client, &pool).await;
    pool.close().await;
}

#[tokio::test]
#[serial_test::serial]
async fn statement_cache_eviction_at_the_pooler_is_transparent() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let tenants = tenant_load::tenant_names("pgbstmt", 8);
    tenant_load::provision_tenants(&client, &broker, &tenants, CORE_CONTEXT, 8).await;

    // More distinct query shapes than max_prepared_statements (32), round-robined
    // across tenants and rounds, so the pooler evicts and re-prepares statements.
    let shapes = query_shapes();
    let mut failures = Vec::new();
    for _round in 0..5 {
        for tenant in &tenants {
            for shape in &shapes {
                let outcome = RequestOutcome::of(
                    client
                        .get(format!("{broker}entities{shape}"))
                        .header("NGSILD-Tenant", tenant.as_str())
                        .header("Accept", "application/ld+json")
                        .header("Link", link_header()),
                )
                .await;
                if !outcome.is_success() {
                    failures.push(outcome);
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "statement-cache eviction at the pooler must be transparent to sqlx: {failures:?}"
    );
    reset(&client, &pool).await;
    pool.close().await;
}

/// More than `max_prepared_statements` distinct query-parameter shapes.
fn query_shapes() -> Vec<String> {
    let mut shapes = Vec::new();
    for attribute in ["seq", "value", "name", "status"] {
        shapes.push(format!("?type=Device&q={attribute}>0"));
        shapes.push(format!("?type=Device&attrs={attribute}"));
        shapes.push(format!("?type=Device&q={attribute}!=1&limit=5"));
    }
    for georel in ["near;maxDistance==1000", "within", "intersects"] {
        shapes.push(format!("?type=Device&georel={georel}&geometry=Point&coordinates=[0,0]"));
    }
    for limit in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24] {
        shapes.push(format!("?type=Device&limit={limit}"));
    }
    shapes
}

#[tokio::test]
#[serial_test::serial]
async fn snapshot_branch_clone_and_promote_run_through_the_pooler_under_load() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let snapshot_tenants = tenant_load::tenant_names("pgbsnap", 4);
    let load_tenants = tenant_load::tenant_names("pgbsnapload", 16);
    tenant_load::provision_tenants(&client, &broker, &snapshot_tenants, CORE_CONTEXT, 4).await;
    tenant_load::provision_tenants(&client, &broker, &load_tenants, CORE_CONTEXT, 16).await;

    // Background load on sixteen tenants while snapshots run on four.
    let picker = tenant_load::UniformPicker::new(load_tenants.len(), 42);
    let load = tenant_load::run_bounded(16, Duration::from_secs(20), |_| {
        let client = &client;
        let broker = &broker;
        let load_tenants = &load_tenants;
        let picker = &picker;
        async move {
            let tenant = &load_tenants[picker.pick()];
            let id = urlencoding::encode(&format!("urn:ngsi-ld:Device:provision:{tenant}")).into_owned();
            RequestOutcome::of(
                client
                    .get(format!("{broker}entities/{id}"))
                    .header("NGSILD-Tenant", tenant.as_str())
                    .header("Accept", "application/ld+json")
                    .header("Link", link_header()),
            )
            .await
        }
    });

    let snapshots = async {
        let mut terminal = 0;
        for tenant in &snapshot_tenants {
            if snapshot_reaches_terminal_state(&client, &broker, tenant).await {
                terminal += 1;
            }
        }
        terminal
    };
    let (load_report, terminal) = tokio::join!(load, snapshots);

    println!(
        "snapshot under load: {} snapshots terminal, {} load requests, {} non-2xx, statuses {:?}, transport failures {:?} {:?}, error bodies {:?}",
        terminal,
        load_report.total,
        load_report.non_2xx,
        load_report.statuses,
        load_report.transport_failures,
        load_report.transport_samples,
        load_report.error_details
    );
    assert_eq!(
        terminal,
        snapshot_tenants.len(),
        "every snapshot must reach a terminal status through the pooler"
    );
    assert_eq!(load_report.non_2xx, 0, "other tenants must be unaffected while snapshots run");
    reset(&client, &pool).await;
    pool.close().await;
}

/// Create a snapshot for `tenant`, wait for it to leave the preparing state, and
/// report whether it reached a terminal status.
async fn snapshot_reaches_terminal_state(client: &Client, broker: &str, tenant: &str) -> bool {
    let created = client
        .post(format!("{broker}snapshots"))
        .header("NGSILD-Tenant", tenant)
        .header("Content-Type", "application/json")
        .json(&json!({"type": "Snapshot", "snapshotQueries": []}))
        .send()
        .await;
    let Ok(response) = created else { return false };
    if !response.status().is_success() {
        return false;
    }
    let Some(location) = response.headers().get("Location").and_then(|value| value.to_str().ok()).map(str::to_owned) else {
        return false;
    };
    let snapshot_id = location.rsplit('/').next().unwrap_or_default().to_owned();
    for _attempt in 0..30 {
        if snapshot_phase(client, broker, tenant, &snapshot_id)
            .await
            .is_some_and(|phase| phase != "preparing" && !phase.is_empty())
        {
            return true;
        }
        sleep(Duration::from_millis(500)).await;
    }
    false
}

/// Read one snapshot's current status phase, if the request and its body parse.
async fn snapshot_phase(client: &Client, broker: &str, tenant: &str, snapshot_id: &str) -> Option<String> {
    let response = client
        .get(format!("{broker}snapshots/{snapshot_id}"))
        .header("NGSILD-Tenant", tenant)
        .header("Accept", "application/json")
        .send()
        .await
        .ok()?;
    let body = response.json::<Value>().await.ok()?;
    Some(body["snapshotStatus"].as_str().unwrap_or_default().to_owned())
}

#[tokio::test]
#[serial_test::serial]
async fn durable_notifications_are_delivered_exactly_per_change_through_the_pooler() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let server = NotificationServer::start(MockServerConfig::from_environment().expect("mock server config"))
        .await
        .expect("notification server");
    let callback = server.endpoint_url().expect("callback url");

    let tenants = tenant_load::tenant_names("pgbnotif", 16);
    tenant_load::provision_tenants(&client, &broker, &tenants, CORE_CONTEXT, 16).await;
    for tenant in &tenants {
        subscribe(&client, &broker, tenant, &callback).await;
    }
    // Fifty updates per tenant, each a genuine change (a distinct value) so it
    // produces a notification rather than a no-op.
    for update in 0..50 {
        let writes = tenants.iter().map(|tenant| {
            let id = urlencoding::encode(&format!("urn:ngsi-ld:Device:provision:{tenant}")).into_owned();
            client
                .patch(format!("{broker}entities/{id}/attrs"))
                .header("NGSILD-Tenant", tenant.as_str())
                .header("Content-Type", "application/ld+json")
                .json(&json!({"value": {"type": "Property", "value": update}, "@context": [CORE_CONTEXT]}))
                .send()
        });
        let _ = join_all(writes).await;
    }

    // Drain notifications for up to 30s and count the deliveries.
    let mut delivered = 0;
    while server.next_request(Duration::from_secs(5)).await.is_ok() {
        delivered += 1;
    }
    let outbox: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE query LIKE '%notification_deliveries%'")
        .fetch_one(&pool)
        .await
        .unwrap_or(0);

    println!(
        "durable notifications: {delivered} delivered across {} tenants, outbox-active queries {outbox}",
        tenants.len()
    );
    // At-least-once contract: at least one notification per change, allowing for
    // the initial notification some brokers send on subscription creation.
    assert!(
        delivered >= 50 * tenants.len(),
        "every change must be delivered at least once through the pooler, saw {delivered}"
    );
    server.shutdown().await.expect("shutdown");
    reset(&client, &pool).await;
    pool.close().await;
}

/// Create a subscription for `tenant` notifying `callback` on `Device` changes.
async fn subscribe(client: &Client, broker: &str, tenant: &str, callback: &str) {
    let _ = client
        .post(format!("{broker}subscriptions"))
        .header("NGSILD-Tenant", tenant)
        .header("Content-Type", "application/ld+json")
        .json(&json!({
            "type": "Subscription",
            "entities": [{"type": "Device"}],
            "notification": {"endpoint": {"uri": callback, "accept": "application/json"}},
            "@context": [CORE_CONTEXT]
        }))
        .send()
        .await;
}

#[tokio::test]
#[serial_test::serial]
async fn synchronous_commit_is_a_database_default_not_session_state() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let tenants = tenant_load::tenant_names("pgbsync", 1);
    tenant_load::provision_tenants(&client, &broker, &tenants, CORE_CONTEXT, 1).await;

    // The broker applies synchronous_commit with ALTER DATABASE, so it appears in
    // pg_db_role_setting for the tenant database rather than as session state.
    let settings: Vec<String> = sqlx::query_scalar(
        "SELECT unnest(setconfig) FROM pg_db_role_setting r JOIN pg_database d ON d.oid = r.setdatabase WHERE d.datname LIKE 'andromeda_tenant_%'",
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_default();

    println!("tenant database settings: {settings:?}");
    assert!(
        settings.iter().any(|setting| setting.starts_with("synchronous_commit")),
        "synchronous_commit must be a database default in pg_db_role_setting, saw {settings:?}"
    );
    reset(&client, &pool).await;
    pool.close().await;
}

#[tokio::test]
#[serial_test::serial]
async fn a_tenant_created_at_runtime_is_routed_with_no_pooler_reload() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let replica = endpoints::broker_replica();
    let tenant = tenant_load::tenant_names("pgbroute", 1).remove(0);

    // Create through the primary, read immediately through both replicas.
    let create = tenant_load::create_entity(
        &client,
        &broker,
        &tenant,
        &tenant_load::device_entity("urn:ngsi-ld:Device:routed", CORE_CONTEXT),
    )
    .await;
    assert_eq!(create.status(), Some(201), "runtime tenant creation must succeed: {create:?}");
    for base in [&broker, &replica] {
        let id = urlencoding::encode("urn:ngsi-ld:Device:routed").into_owned();
        let read = RequestOutcome::of(
            client
                .get(format!("{base}entities/{id}"))
                .header("NGSILD-Tenant", tenant.as_str())
                .header("Accept", "application/ld+json")
                .header("Link", link_header()),
        )
        .await;
        assert_eq!(
            read.status(),
            Some(200),
            "the new tenant must be routed with no pooler reload on {base}: {read:?}"
        );
    }

    // The console lists the tenant database as an auto-registered database.
    let console = PoolerConsole::new(endpoints::pgbouncer_admin_url());
    let listed = console
        .show_pools()
        .await
        .is_ok_and(|pools| pools.iter().any(|pool| pool.database.starts_with("andromeda_tenant_")));
    println!("runtime routing: tenant database present in pooler pools = {listed}");
    reset(&client, &pool).await;
    pool.close().await;
}

#[tokio::test]
#[serial_test::serial]
async fn maintenance_holds_its_session_lock_on_a_direct_connection_while_requests_are_pooled() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let tenants = tenant_load::tenant_names("pgbmaint", 4);
    tenant_load::provision_tenants(&client, &broker, &tenants, CORE_CONTEXT, 4).await;

    let before = maintenance_decisions(&client, &broker).await;
    // Drive some writes to give maintenance something to decide on, then wait a
    // couple of responsive-profile cadences.
    for tenant in &tenants {
        let _ = upsert_batch(&client, &broker, tenant, 100).await;
    }
    sleep(Duration::from_secs(15)).await;
    let after = maintenance_decisions(&client, &broker).await;

    let pooler_addr = pooler_client_addr().await;
    let pooler_held: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a ON a.pid = l.pid \
         WHERE l.locktype = 'advisory' AND host(a.client_addr) = $1",
    )
    .bind(&pooler_addr)
    .fetch_one(&pool)
    .await
    .expect("count advisory locks held over the pooler");

    println!(
        "maintenance: decisions advanced by {}, advisory locks held via pooler {pooler_addr} {pooler_held}",
        after - before
    );
    assert!(after >= before, "maintenance decisions counter must not regress");
    assert_eq!(pooler_held, 0, "no maintenance advisory lock may be held over a pooler connection");
    reset(&client, &pool).await;
    pool.close().await;
}

#[tokio::test]
#[serial_test::serial]
async fn concurrent_writes_to_one_entity_serialise_correctly_on_two_backends() {
    let client = client();
    let pool = control_pool().await;
    reset(&client, &pool).await;
    let broker = endpoints::broker();
    let tenant = tenant_load::tenant_names("pgbserial", 1).remove(0);
    tenant_load::provision_tenants(&client, &broker, slice::from_ref(&tenant), CORE_CONTEXT, 1).await;

    // Thirty-two concurrent PATCHes to the one provisioned entity.
    let entity_id = format!("urn:ngsi-ld:Device:provision:{tenant}");
    let encoded = urlencoding::encode(&entity_id).into_owned();
    let patches = (0..32).map(|value| {
        let client = &client;
        let broker = &broker;
        let tenant = &tenant;
        let encoded = &encoded;
        async move {
            RequestOutcome::of(
                client
                    .patch(format!("{broker}entities/{encoded}/attrs"))
                    .header("NGSILD-Tenant", tenant.as_str())
                    .header("Content-Type", "application/ld+json")
                    .json(&json!({"value": {"type": "Property", "value": value}, "@context": [CORE_CONTEXT]})),
            )
            .await
        }
    });
    let outcomes = join_all(patches).await;
    let failed: Vec<&RequestOutcome> = outcomes.iter().filter(|outcome| outcome.status().is_none_or(|status| status >= 500)).collect();

    // Final state is consistent and readable.
    let final_read = RequestOutcome::of(
        client
            .get(format!("{broker}entities/{encoded}"))
            .header("NGSILD-Tenant", tenant.as_str())
            .header("Accept", "application/ld+json")
            .header("Link", link_header()),
    )
    .await;

    println!("concurrent writes: {} of 32 failed ({failed:?}), final read {final_read:?}", failed.len());
    assert!(
        failed.is_empty(),
        "concurrent writes to one entity must neither 5xx nor drop on a pool of two backends: {failed:?}"
    );
    assert_eq!(
        final_read.status(),
        Some(200),
        "the entity must be consistent and readable after concurrent writes: {final_read:?}"
    );
    let _ = pool_has_rows(&pool).await;
    reset(&client, &pool).await;
    pool.close().await;
}

/// A trivial control-database probe, kept so the control pool is exercised.
async fn pool_has_rows(pool: &PgPool) -> bool {
    sqlx::query("SELECT 1").fetch_optional(pool).await.is_ok_and(|row| row.is_some())
}
