//! First-touch tenant provisioning is robust under concurrency.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.14 lets an NGSI-LD system serve several
//! tenants, each materialised on first use. Andromeda provisions a fresh
//! per-tenant database on the first create under a new `NGSILD-Tenant`, and
//! several such creates arriving together contend on PostgreSQL's `CREATE
//! DATABASE` (SQLSTATE 55006 `object_in_use`, and defensively 40001/40P01/53300),
//! with a lost race to an identical concurrent create surfacing as 42P04. A valid
//! create must not surface `InternalError` because provisioning raced: the broker
//! retries the transient states and treats the lost race as success, so every
//! concurrent first-touch create succeeds and every tenant is independently
//! usable afterwards.
//!
//! The collection of tenant databases is broker-wide, so this runs `#[serial]`
//! and sweeps `andromeda_tenant_%` databases on entry and exit.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::{configuration::RUNTIME_BROKER_BASE as BASE_URL, database_access::connect_test_database};
use futures::future::join_all;
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

/// How many distinct fresh tenants provision concurrently. Enough to force
/// several simultaneous first-touch `CREATE DATABASE` statements to contend.
const TENANT_COUNT: usize = 12;

/// A minimal `Device` entity carrying one property, under the core context.
fn device_entity(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Device",
        "value": {"type": "Property", "value": 1},
        "@context": [CORE_CONTEXT]
    })
}

/// Create one entity under `tenant`, returning the response status.
async fn create_under_tenant(client: &Client, tenant: &str, id: &str) -> StatusCode {
    client
        .post(format!("{BASE_URL}/entities"))
        .header("NGSILD-Tenant", tenant)
        .header("Content-Type", "application/ld+json")
        .json(&device_entity(id))
        .send()
        .await
        .map_or(StatusCode::SERVICE_UNAVAILABLE, |response| response.status())
}

/// Retrieve one entity under `tenant`, returning the response status.
async fn retrieve_under_tenant(client: &Client, tenant: &str, id: &str) -> StatusCode {
    let encoded = urlencoding::encode(id);
    client
        .get(format!("{BASE_URL}/entities/{encoded}"))
        .header("NGSILD-Tenant", tenant)
        .header("Accept", "application/ld+json")
        .header(
            "Link",
            format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        )
        .send()
        .await
        .map_or(StatusCode::SERVICE_UNAVAILABLE, |response| response.status())
}

/// The shared name prefix of every tenant this suite provisions, so its cleanup
/// touches only its own tenants and never a concurrent suite's.
const TENANT_PREFIX: &str = "tenantschema";

/// Suspend the tenants this suite created so the broker closes their per-tenant
/// pools before the databases are dropped, ignoring the outcome (a best-effort
/// cleanup step; the force drop closes any backend a suspend did not, or that a
/// broker without the tenant-management extension left open).
async fn suspend_tenants(client: &Client) {
    let Ok(response) = client.get(format!("{BASE_URL}/tenants")).send().await else {
        return;
    };
    let Ok(body) = response.json::<serde_json::Value>().await else {
        return;
    };
    let names: Vec<String> = body
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tenant| tenant.get("tenantName").and_then(serde_json::Value::as_str))
        .filter(|name| name.starts_with(TENANT_PREFIX))
        .map(str::to_owned)
        .collect();
    for name in names {
        let _ = client
            .patch(format!("{BASE_URL}/tenants/{name}"))
            .header("Content-Type", "application/json")
            .body("{\"isActive\":false}")
            .send()
            .await;
    }
}

/// Drop only this suite's own tenant databases (named `andromeda_tenant_<TENANT_PREFIX>...`),
/// forcing off the permanent background-worker backends a broker pool close
/// cannot remove. Scoping to the suite's own prefix keeps this cleanup from
/// dropping a concurrent suite's tenant databases.
async fn drop_tenant_databases(pool: &PgPool) {
    let current: String = sqlx::query_scalar("SELECT current_database()").fetch_one(pool).await.unwrap_or_default();
    let like = format!("andromeda_tenant_{TENANT_PREFIX}%");
    let databases: Vec<String> = sqlx::query_scalar("SELECT datname FROM pg_database WHERE datname LIKE $1 AND datname <> $2")
        .bind(&like)
        .bind(&current)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    for database in databases {
        let statement = format!("DROP DATABASE IF EXISTS \"{database}\" WITH (FORCE)");
        let _ = sqlx::query(sqlx::AssertSqlSafe(statement)).execute(pool).await;
    }
}

/// Clause 4.14: concurrent first-touch creates under distinct fresh tenants each
/// provision their tenant database without any surfacing `InternalError`, and
/// every tenant is independently retrievable afterwards.
#[tokio::test]
#[serial_test::serial]
async fn concurrent_first_touch_tenant_creation_never_500s() {
    let client = Client::builder().timeout(Duration::from_secs(30)).build().expect("reqwest client");
    let pool = connect_test_database().await.expect("connect to PostgreSQL directly");

    // Start from a clean slate in case a prior run leaked tenant databases.
    suspend_tenants(&client).await;
    drop_tenant_databases(&pool).await;

    let tenants: Vec<(String, String)> = (0..TENANT_COUNT)
        .map(|index| {
            let tenant = format!("tenantschema{}", Uuid::new_v4().simple());
            let id = format!("urn:ngsi-ld:Device:concurrent-provisioning:{index}");
            (tenant, id)
        })
        .collect();

    // Fire every first-touch create at once so their CREATE DATABASE statements
    // contend, which is exactly what the provisioning retry must tolerate.
    let creates = tenants.iter().map(|(tenant, id)| {
        let client = client.clone(); // clone: each task owns its handle; the inner reqwest pool is shared.
        async move { create_under_tenant(&client, tenant, id).await }
    });
    let create_statuses = join_all(creates).await;
    for (status, (tenant, _)) in create_statuses.iter().zip(&tenants) {
        assert_eq!(*status, StatusCode::CREATED, "first-touch create for {tenant} must not surface a 5xx");
    }

    // Each tenant's data is independently readable, proving the tenant database
    // was actually provisioned rather than merely not erroring.
    let reads = tenants.iter().map(|(tenant, id)| {
        let client = client.clone(); // clone: each task owns its handle; the inner reqwest pool is shared.
        async move { retrieve_under_tenant(&client, tenant, id).await }
    });
    let read_statuses = join_all(reads).await;
    for (status, (tenant, _)) in read_statuses.iter().zip(&tenants) {
        assert_eq!(*status, StatusCode::OK, "tenant {tenant} must be independently retrievable after provisioning");
    }

    suspend_tenants(&client).await;
    drop_tenant_databases(&pool).await;
    pool.close().await;
}
