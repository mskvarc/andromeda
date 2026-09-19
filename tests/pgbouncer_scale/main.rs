//! Transaction-pooling scale suite: hundreds of tenants served through `PgBouncer`.
//!
//! Runs against the `pgbouncer` stack, where PostgreSQL's `max_connections` (500)
//! is far below the broker's un-pooled request demand: at the runtime-cache cap
//! the cached working set alone is `runtime_max_cached` * `tenant_max_connections`
//! = 64 * 16 = 1024 backend connections, and the whole request path up to
//! `database.max_connections` + that = 1056. Without the pooler the working set
//! alone exhausts PostgreSQL and requests fail with `PoolTimedOut`; through it,
//! each active tenant's up-to-16 client connections multiplex onto only
//! `default_pool_size` (2) PostgreSQL backends, so the request-path footprint is
//! a small multiple of the cache size and every tenant is served under the ceiling.
//!
//! The suite provisions hundreds of tenant databases (each created and migrated
//! over the direct base, then served through the pooler), hammers the cached
//! working set concurrently to drive request-path connection pressure, confirms
//! every tenant remains reachable through the wildcard route, and asserts that
//! PostgreSQL's client-backend count for the working set stayed far under its
//! un-pooled demand.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::{configuration::RUNTIME_BROKER_BASE as BASE_URL, database_access::connect_test_database};
use futures::stream::{self, StreamExt};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{env, future::Future, time::Duration};
use tokio::time::sleep;
use uuid::Uuid;

/// Number of distinct tenants the suite provisions. Overridable so the tier can
/// be pushed harder without an edit; the default is a few hundred.
fn tenant_count() -> usize {
    env::var("PGBOUNCER_SCALE_TENANTS").ok().and_then(|value| value.parse().ok()).unwrap_or(200)
}

/// Concurrent provisioning requests. Bounded because provisioning bypasses the
/// pooler (`CREATE DATABASE` and the migrator's session lock reach PostgreSQL
/// directly), so this many direct connections are open at once, not the pooled
/// request path.
const PROVISION_CONCURRENCY: usize = 12;

/// Concurrent requests during the steady-state hammer. This many request-path
/// transactions are in flight at once, all multiplexed by the pooler.
const STEADY_CONCURRENCY: usize = 48;

/// Reads issued against each tenant in the cached working set during the hammer.
const HAMMER_ROUNDS: usize = 10;

/// A ceiling on PostgreSQL client backends, comfortably under the stack's
/// `max_connections` (500) and far under the broker's un-pooled working-set
/// demand (64 * 16 = 1024). With the pooler, the working set's backends are
/// bounded by `cached_tenants` * `default_pool_size` plus platform and harness
/// connections, which stays well below this.
const CLIENT_BACKEND_CEILING: i64 = 280;

fn link_header() -> String {
    format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

fn vehicle_entity(id: &str, name: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "name": {"type": "Property", "value": name},
        "@context": [CORE_CONTEXT]
    })
}

/// POST an entity under `tenant`, returning the response status.
async fn create_entity(client: &Client, tenant: &str, id: &str, name: &str) -> StatusCode {
    client
        .post(format!("{BASE_URL}/entities"))
        .header("NGSILD-Tenant", tenant)
        .header("Content-Type", "application/ld+json")
        .json(&vehicle_entity(id, name))
        .send()
        .await
        .map_or(StatusCode::SERVICE_UNAVAILABLE, |response| response.status())
}

/// GET an entity under `tenant`, returning the response status.
async fn get_entity(client: &Client, tenant: &str, id: &str) -> StatusCode {
    client
        .get(format!("{BASE_URL}/entities/{id}"))
        .header("NGSILD-Tenant", tenant)
        .header("Accept", "application/ld+json")
        .header("Link", link_header())
        .send()
        .await
        .map_or(StatusCode::SERVICE_UNAVAILABLE, |response| response.status())
}

/// POST `/tenants/evictOrphans` so the broker closes every orphaned tenant pool.
async fn evict_orphans(client: &Client) {
    let response = client
        .post(format!("{BASE_URL}/tenants/evictOrphans"))
        .send()
        .await
        .expect("POST /tenants/evictOrphans failed - is the broker running?");
    assert!(response.status().is_success(), "POST /tenants/evictOrphans returned {}", response.status());
}

/// Drop every `andromeda_tenant_*` database with `FORCE`, killing the residual
/// `TimescaleDB` background-worker backend each one holds.
async fn drop_tenant_databases(pool: &PgPool) {
    let current: String = sqlx::query_scalar("SELECT current_database()").fetch_one(pool).await.unwrap_or_default();
    let databases: Vec<String> = sqlx::query_scalar("SELECT datname FROM pg_database WHERE datname LIKE 'andromeda_tenant_%' AND datname <> $1")
        .bind(&current)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    for database in databases {
        let statement = format!("DROP DATABASE IF EXISTS \"{database}\" WITH (FORCE)");
        let _ = sqlx::query(sqlx::AssertSqlSafe(statement)).execute(pool).await;
    }
}

/// Count PostgreSQL client backends, the connections the pooler multiplexes onto.
/// Background workers (one per touched tenant database) are a separate
/// `backend_type` and are excluded.
async fn client_backend_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE backend_type = 'client backend'")
        .fetch_one(pool)
        .await
        .expect("read pg_stat_activity client backend count")
}

/// Run one closure per (tenant, entity id) pair with bounded concurrency,
/// collecting the failure messages the closures return.
async fn run_bounded<F, Fut>(items: Vec<(String, String)>, concurrency: usize, operation: F) -> Vec<String>
where
    F: Fn((String, String)) -> Fut + Send + Sync,
    Fut: Future<Output = Option<String>> + Send,
{
    stream::iter(items)
        .map(operation)
        .buffer_unordered(concurrency)
        .filter_map(|failure| async move { failure })
        .collect()
        .await
}

/// Render up to the first ten failures for an assertion message.
fn sample(failures: &[String]) -> String {
    let shown = &failures[..failures.len().min(10)];
    format!("{} failure(s); first {}: {shown:?}", failures.len(), shown.len())
}

#[serial_test::serial]
#[tokio::test]
async fn pgbouncer_scale_hundreds_of_tenants_are_served_under_the_connection_ceiling() {
    let tenants_total = tenant_count();
    let client = Client::builder().timeout(Duration::from_secs(30)).build().expect("reqwest client");
    let pool = connect_test_database().await.expect("connect to PostgreSQL directly");

    // Start from a clean slate: close any orphaned pools, drop leftover tenant DBs.
    evict_orphans(&client).await;
    drop_tenant_databases(&pool).await;

    // Distinct tenants, each with one entity id.
    let tenants: Vec<(String, String)> = (0..tenants_total)
        .map(|index| (format!("pgbscale{}", Uuid::new_v4().simple()), format!("urn:ngsi-ld:Vehicle:pgbscale:{index}")))
        .collect();

    // Phase 1 - provision every tenant: POST creates the tenant database (direct)
    // and writes the entity (pooled); GET reads it back (pooled). Bounded so the
    // direct provisioning path stays within PostgreSQL's ceiling.
    let provision_failures = run_bounded(tenants.clone(), PROVISION_CONCURRENCY, |(tenant, id)| {
        let client = client.clone();
        async move {
            let created = create_entity(&client, &tenant, &id, "provisioned").await;
            if created != StatusCode::CREATED {
                return Some(format!("create[{tenant}] -> {created}"));
            }
            let read = get_entity(&client, &tenant, &id).await;
            if read != StatusCode::OK {
                return Some(format!("read-back[{tenant}] -> {read}"));
            }
            None
        }
    })
    .await;
    assert!(provision_failures.is_empty(), "provisioning: {}", sample(&provision_failures));

    // Let the provisioning burst drain. Provisioning touched every tenant in
    // quick succession, so the pooler briefly held a server pool for each; a wait
    // longer than server_idle_timeout releases those, leaving the measurement
    // below reflecting only the cached working set the next phase keeps live.
    sleep(Duration::from_secs(8)).await;

    // Phase 2 - hammer the cached working set: read the last RUNTIME_MAX_CACHED
    // tenants (the most recently provisioned, so still in the broker's runtime
    // cache) repeatedly at high concurrency, so their serving pools stay live and
    // open many concurrent connections to the pooler at once. Without the pooler
    // this is where the request path exhausts PostgreSQL: 64 cached tenants times
    // tenant_max_connections (16) is 1024 backends, twice PostgreSQL's ceiling.
    let working_set: Vec<(String, String)> = tenants.iter().rev().take(64.min(tenants_total)).cloned().collect();
    let hammer: Vec<(String, String)> = working_set.iter().cloned().cycle().take(working_set.len() * HAMMER_ROUNDS).collect();
    let hammer_failures = run_bounded(hammer, STEADY_CONCURRENCY, |(tenant, id)| {
        let client = client.clone();
        async move {
            let read = get_entity(&client, &tenant, &id).await;
            if read == StatusCode::OK {
                None
            } else {
                Some(format!("hammer-read[{tenant}] -> {read}"))
            }
        }
    })
    .await;
    assert!(hammer_failures.is_empty(), "steady hammer: {}", sample(&hammer_failures));

    // Sample the connection ceiling while the serving pools are still warm.
    let client_backends = client_backend_count(&pool).await;
    assert!(
        client_backends < CLIENT_BACKEND_CEILING,
        "serving the cached working set through the pooler must hold far fewer PostgreSQL backends than the un-pooled request path would (64 tenants * 16 = 1024), saw {client_backends}"
    );

    // Phase 3 - confirm every tenant, including those long evicted from the
    // runtime cache, is still reachable through the wildcard route with no reload.
    let reachability_failures = run_bounded(tenants.clone(), PROVISION_CONCURRENCY, |(tenant, id)| {
        let client = client.clone();
        async move {
            let read = get_entity(&client, &tenant, &id).await;
            if read == StatusCode::OK {
                None
            } else {
                Some(format!("reachable[{tenant}] -> {read}"))
            }
        }
    })
    .await;
    assert!(reachability_failures.is_empty(), "reachability sweep: {}", sample(&reachability_failures));

    eprintln!(
        "pgbouncer_scale: {tenants_total} tenants served; cached working set held {client_backends} PostgreSQL client backends (ceiling {CLIENT_BACKEND_CEILING}; the un-pooled working set alone would be 64 * 16 = 1024)"
    );

    // Cleanup: close pools, drop every tenant database.
    evict_orphans(&client).await;
    drop_tenant_databases(&pool).await;
    pool.close().await;
}
