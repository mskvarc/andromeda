//! Multitenancy integration tests using the standard Rust harness.
//!
//! Verifies tenant isolation, implicit tenant creation, cross-tenant visibility
//! rules, and notification header propagation per the NGSI-LD multitenancy
//! specification (ETSI GS CIM 009 clauses 4.14, 5.5.10 and 6.3.14).
//!
//! Naming convention: `multitenancy_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::{context::CORE_CONTEXT, problem_detail as error};
use andromeda_test_harness::{
    configuration::RUNTIME_BROKER_BASE as BASE_URL,
    database_access::connect_test_database,
    serialized_environment::SerializedEnvironment,
};
use reqwest::{Client, Method, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

#[path = "../support/notification_adapter.rs"]
pub mod notification_adapter;
use notification_adapter::NotificationAdapter as NotificationReceiver;

const NONEXISTENT_TENANT_ERROR: &iri_rs::Iri<&str> = error::NONEXISTENT_TENANT;

fn client() -> Client {
    Client::new()
}

fn link_header() -> String {
    format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

/// Delete every named-tenant registry row so the broker's next orphan
/// sweep closes the per-tenant pools they refer to.
async fn clean_named_tenant_registry(main_pool: &PgPool) {
    let has_tenants_table: bool = sqlx::query_scalar("SELECT to_regclass('public.tenants') IS NOT NULL")
        .fetch_one(main_pool)
        .await
        .unwrap_or(false);
    if !has_tenants_table {
        return;
    }
    let _ = sqlx::query("DELETE FROM tenants WHERE is_unnamed_default = false").execute(main_pool).await;
}

#[must_use = "finish the multitenancy environment so tenant databases and their background workers are removed"]
struct MultitenancyEnvironment {
    serialized: SerializedEnvironment,
    cleanup_pool: PgPool,
}

impl MultitenancyEnvironment {
    async fn finish(mut self) {
        suspend_all_tenants().await;
        clean_named_tenant_registry(&self.cleanup_pool).await;
        drop_orphan_tenant_databases(&self.cleanup_pool).await;
        self.serialized.reset().await.expect("serialized test database final reset failed");
        self.cleanup_pool.close().await;
        self.serialized.finish().await.expect("serialized test database release failed");
    }
}

async fn reset_broker_state() -> MultitenancyEnvironment {
    let mut environment = SerializedEnvironment::from_environment_reset()
        .await
        .expect("serialized test database environment");
    let pool = connect_test_database().await.expect("connect to test database");
    // Order: suspend every tenant first, while its registry row still exists, so
    // the broker closes its sqlx pools cleanly; then remove the rows; then
    // DROP DATABASE to kill the per-tenant TimescaleDB background-worker
    // processes that otherwise hold a `max_connections` slot each for the
    // lifetime of the database. TRUNCATE must run last on a server with
    // free slots.
    suspend_all_tenants().await;
    clean_named_tenant_registry(&pool).await;
    drop_orphan_tenant_databases(&pool).await;
    environment.reset().await.expect("serialized test database reset failed");
    MultitenancyEnvironment {
        serialized: environment,
        cleanup_pool: pool,
    }
}

/// Suspend every named tenant so the broker closes its per-tenant pools before
/// the registry rows are removed and the databases dropped.
///
/// Best-effort: a broker without the tenant-management extension lists nothing,
/// and the subsequent force drop closes any backend a suspend did not, so a
/// silent failure here does not strand the cleanup.
async fn suspend_all_tenants() {
    let Ok(response) = client().get(format!("{BASE_URL}/tenants")).send().await else {
        return;
    };
    let Ok(body) = response.json::<Value>().await else {
        return;
    };
    let names: Vec<String> = body
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tenant| tenant.get("tenantName").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    for name in names {
        let _ = client()
            .patch(format!("{BASE_URL}/tenants/{name}"))
            .header("Content-Type", "application/json")
            .body("{\"isActive\":false}")
            .send()
            .await;
    }
}

/// Drop every `andromeda_tenant_*` database (except the current one).
///
/// Each tenant database, once touched, spawns a permanent
/// `TimescaleDB Background Worker Scheduler` backend that holds a
/// `max_connections` slot until the database is dropped. The broker's
/// `pool.close()` cannot remove it — only `DROP DATABASE` does. `WITH
/// (FORCE)` (PostgreSQL 13+) terminates any residual backends so the drop
/// succeeds even if the broker's sqlx pool has not yet fully released.
async fn drop_orphan_tenant_databases(main_pool: &PgPool) {
    let current: String = sqlx::query_scalar("SELECT current_database()").fetch_one(main_pool).await.unwrap_or_default();
    let dbs: Vec<String> = sqlx::query_scalar("SELECT datname FROM pg_database WHERE datname LIKE 'andromeda_tenant_%' AND datname <> $1")
        .bind(&current)
        .fetch_all(main_pool)
        .await
        .unwrap_or_default();
    for db in dbs {
        let sql = format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)");
        let _ = sqlx::query(sqlx::AssertSqlSafe(sql)).execute(main_pool).await;
    }
}

fn unique_entity_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

fn unique_subscription_id(case_id: &str) -> String {
    format!("urn:ngsi-ld:Subscription:{case_id}:{}", Uuid::new_v4())
}

async fn read_json_or_null(response: reqwest::Response) -> Value {
    match response.json::<Value>().await {
        Ok(body) => body,
        Err(_) => Value::Null,
    }
}

/// Sends an NGSI-LD request. An empty `tenant` string omits the `NGSILD-Tenant` header.
async fn ngsi_request(
    http_client: &Client,
    method: Method,
    path: &str,
    body: Option<&Value>,
    tenant: &str,
    accept_ld_json: bool,
) -> (StatusCode, HeaderMap, Value) {
    let mut request = http_client.request(method, format!("{BASE_URL}{path}"));

    if accept_ld_json {
        request = request.header("Accept", "application/ld+json").header("Link", link_header());
    }

    if !tenant.is_empty() {
        request = request.header("NGSILD-Tenant", tenant);
    }

    if let Some(body) = body {
        request = request.header("Content-Type", "application/ld+json").json(body);
    }

    let response = request.send().await.expect("ngsi request failed");
    let status = response.status();
    let headers = response.headers().clone();
    let response_body = read_json_or_null(response).await;
    (status, headers, response_body)
}

/// Sends a plain JSON body, as `POST /jsonldContexts` and a batch delete take
/// it, to `tenant`.
async fn json_request(http_client: &Client, method: Method, path: &str, body: &Value, tenant: &str) -> (StatusCode, HeaderMap, Value) {
    let response = http_client
        .request(method, format!("{BASE_URL}{path}"))
        .header("NGSILD-Tenant", tenant)
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .expect("json request failed");
    let status = response.status();
    let headers = response.headers().clone();
    let response_body = read_json_or_null(response).await;
    (status, headers, response_body)
}

/// Assert that `tenant` exists: `path` on it answers 200 and echoes the tenant
/// (clause 6.3.14).
async fn assert_tenant_serves(http_client: &Client, path: &str, tenant: &str) {
    let (status, headers, body) = ngsi_request(http_client, Method::GET, path, None, tenant, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK, "GET {path} on {tenant}: {body}");
    pretty_assertions::assert_eq!(
        headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()),
        Some(tenant),
        "GET {path} on {tenant} must echo NGSILD-Tenant"
    );
}

/// Assert that a request refused on `tenant` was refused as `NonexistentTenant`
/// with the tenant echoed, and that the tenant still does not exist afterwards.
async fn assert_refused_without_creating(http_client: &Client, operation: &str, refusal: (StatusCode, HeaderMap, Value), tenant: &str) {
    let (status, headers, body) = refusal;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND, "{operation} on unknown {tenant}: {body}");
    pretty_assertions::assert_eq!(
        body["type"],
        NONEXISTENT_TENANT_ERROR.as_str(),
        "{operation} is not among the clause 5.5.10 creating operations, so it must raise NonexistentTenant"
    );
    pretty_assertions::assert_eq!(
        headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()),
        Some(tenant),
        "{operation} refusal must echo NGSILD-Tenant"
    );
    let (status, _, body) = ngsi_request(http_client, Method::GET, "/entities?type=Vehicle", None, tenant, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND, "{operation} must not create {tenant}: {body}");
    pretty_assertions::assert_eq!(body["type"], NONEXISTENT_TENANT_ERROR.as_str(), "{operation} must leave {tenant} nonexistent");
}

/// GET /tenants and decode JSON body.
async fn list_tenants(http_client: &Client) -> (StatusCode, Value) {
    let response = http_client
        .get(format!("{BASE_URL}/tenants"))
        .send()
        .await
        .expect("list_tenants request failed");
    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}

fn vehicle_entity(id: &str, name: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "name": {"type": "Property", "value": name},
        "@context": [CORE_CONTEXT]
    })
}

fn temporal_vehicle_entity(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "values": [[55, "2020-08-01T12:00:00Z"]]
        },
        "@context": [CORE_CONTEXT]
    })
}

fn entity_subscription(id: &str, endpoint_url: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {
            "endpoint": {"uri": endpoint_url, "accept": "application/json"}
        },
        "@context": [CORE_CONTEXT]
    })
}

fn csr_registration(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [
            {
                "entities": [{"id": "urn:ngsi-ld:Vehicle:A456", "type": "Vehicle"}],
                "propertyNames": ["brandName", "speed"],
                "relationshipNames": ["isParked"]
            }
        ],
        "endpoint": "http://my.csource.org:1026",
        "@context": [CORE_CONTEXT]
    })
}

fn csr_subscription(id: &str, endpoint_url: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {
            "endpoint": {"uri": endpoint_url, "accept": "application/json"}
        },
        "@context": [CORE_CONTEXT]
    })
}

// --- Tests ---

#[serial_test::serial]
#[tokio::test]
async fn multitenancy_unnamed_and_named_default_tenants_are_isolated() {
    let http_client = client();
    let environment = reset_broker_state().await;

    let id = unique_entity_id("urn:ngsi-ld:Vehicle:tenant-isolation");
    let unnamed_entity = vehicle_entity(&id, "unnamed-default");
    let named_entity = vehicle_entity(&id, "named-default");

    // Create in unnamed default tenant (no NGSILD-Tenant header)
    let (status, create_headers, _) = ngsi_request(&http_client, Method::POST, "/entities", Some(&unnamed_entity), "", false).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    assert!(
        create_headers.get("NGSILD-Tenant").is_none(),
        "unnamed default tenant response must omit NGSILD-Tenant header"
    );

    // Create same ID in named "default" tenant
    let (status, create_headers, _) = ngsi_request(&http_client, Method::POST, "/entities", Some(&named_entity), "default", false).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(
        create_headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()),
        Some("default"),
        "named default tenant response must include NGSILD-Tenant header"
    );

    // Retrieve from unnamed default → should see unnamed version
    let (status, _, body) = ngsi_request(&http_client, Method::GET, &format!("/entities/{id}"), None, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["name"]["value"], "unnamed-default");

    // Retrieve from named "default" tenant → should see named version
    let (status, _, body) = ngsi_request(&http_client, Method::GET, &format!("/entities/{id}"), None, "default", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["name"]["value"], "named-default");

    environment.finish().await;
}

#[serial_test::serial]
#[tokio::test]
async fn multitenancy_non_create_on_unknown_tenant_returns_nonexistent_tenant_error() {
    let http_client = client();
    let environment = reset_broker_state().await;

    let tenant = format!("unknown-tenant-{}", Uuid::new_v4());
    let id = unique_entity_id("urn:ngsi-ld:Vehicle:noncreate");

    let (status, resp_headers, body) = ngsi_request(&http_client, Method::GET, &format!("/entities/{id}"), None, &tenant, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(
        body["type"],
        NONEXISTENT_TENANT_ERROR.as_str(),
        "accessing unknown tenant via non-create operation must return NonexistentTenant"
    );
    pretty_assertions::assert_eq!(
        resp_headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()),
        Some(tenant.as_str()),
        "response must echo NGSILD-Tenant header"
    );

    environment.finish().await;
}

/// Clause 5.5.10 enumerates the six operations that may create a Tenant
/// implicitly; "All other NGSI-LD operations [...] that target a non-existing
/// Tenant should raise an error of type `NonexistentTenant`". Batch Upsert
/// (5.6.8) and Batch Merge (5.6.20) can create Entities but are not in the list,
/// and neither is Add @context (5.13.2), so each is refused and the Tenant is
/// not created as a side effect.
#[serial_test::serial]
#[tokio::test]
async fn multitenancy_non_creating_operations_on_unknown_tenant_raise_nonexistent_tenant_and_create_nothing() {
    let http_client = client();
    let environment = reset_broker_state().await;

    let id = unique_entity_id("urn:ngsi-ld:Vehicle:noncreating");
    let batch = json!([vehicle_entity(&id, "batch")]);
    let fragment = json!({"name": {"type": "Property", "value": "fragment"}, "@context": [CORE_CONTEXT]});
    let attrs_path = format!("/entities/{id}/attrs");
    let entity_path = format!("/entities/{id}");
    let ld_json_operations: [(&str, Method, &str, Option<&Value>); 5] = [
        ("Batch Entity Upsert (5.6.8)", Method::POST, "/entityOperations/upsert", Some(&batch)),
        ("Batch Entity Merge (5.6.20)", Method::POST, "/entityOperations/merge", Some(&batch)),
        ("Update Attributes (5.6.2)", Method::PATCH, &attrs_path, Some(&fragment)),
        ("Append Attributes (5.6.3)", Method::POST, &attrs_path, Some(&fragment)),
        ("Delete Entity (5.6.6)", Method::DELETE, &entity_path, None),
    ];
    for (operation, method, path, body) in ld_json_operations {
        let tenant = format!("tenant-noncreating-{}", Uuid::new_v4());
        let refusal = ngsi_request(&http_client, method, path, body, &tenant, false).await;
        assert_refused_without_creating(&http_client, operation, refusal, &tenant).await;
    }

    let json_operations = [
        ("Batch Entity Delete (5.6.10)", "/entityOperations/delete", json!([id])),
        (
            "Add @context (5.13.2)",
            "/jsonldContexts",
            json!({"@context": {"noncreating": "https://example.org/noncreating"}}),
        ),
    ];
    for (operation, path, body) in json_operations {
        let tenant = format!("tenant-noncreating-{}", Uuid::new_v4());
        let refusal = json_request(&http_client, Method::POST, path, &body, &tenant).await;
        assert_refused_without_creating(&http_client, operation, refusal, &tenant).await;
    }

    environment.finish().await;
}

/// Batch Entity Creation (5.6.7) is one of the clause 5.5.10 creating
/// operations, and clause 5.6.7.4 fails a refused entity on its own, so a batch
/// in which one entity is refused still answers 207 and still creates the Tenant
/// for the entity it created.
#[serial_test::serial]
#[tokio::test]
async fn multitenancy_batch_create_with_a_refused_entity_still_creates_the_tenant() {
    let http_client = client();
    let environment = reset_broker_state().await;

    let tenant = format!("tenant-batch-partial-{}", Uuid::new_v4());
    let created = unique_entity_id("urn:ngsi-ld:Vehicle:batch-partial-created");
    let refused = unique_entity_id("urn:ngsi-ld:Vehicle:batch-partial-refused");
    // A null Property value is refused per entity (clause 5.5.4).
    let refused_entity = json!({
        "id": refused,
        "type": "Vehicle",
        "name": {"type": "Property", "value": null},
        "@context": [CORE_CONTEXT]
    });
    let batch = json!([vehicle_entity(&created, "created"), refused_entity]);
    let (status, headers, body) = ngsi_request(&http_client, Method::POST, "/entityOperations/create", Some(&batch), &tenant, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS, "{body}");
    pretty_assertions::assert_eq!(headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant.as_str()));
    pretty_assertions::assert_eq!(body["success"], json!([created]));
    pretty_assertions::assert_eq!(body["errors"][0]["entityId"], json!(refused));

    assert_tenant_serves(&http_client, &format!("/entities/{created}"), &tenant).await;
    let (status, _, _) = ngsi_request(&http_client, Method::GET, &format!("/entities/{refused}"), None, &tenant, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND, "the refused entity was not created");

    environment.finish().await;
}

#[serial_test::serial]
#[tokio::test]
async fn multitenancy_create_operations_implicitly_create_named_tenant() {
    let http_client = client();
    let environment = reset_broker_state().await;

    // 1. Create Entity
    let tenant_1 = format!("tenant-create-entity-{}", Uuid::new_v4());
    let id_1 = unique_entity_id("urn:ngsi-ld:Vehicle:create-entity");
    let (status, headers, _) = ngsi_request(&http_client, Method::POST, "/entities", Some(&vehicle_entity(&id_1, "v1")), &tenant_1, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant_1.as_str()));
    assert_tenant_serves(&http_client, &format!("/entities/{id_1}"), &tenant_1).await;
    // Entity not visible in unnamed default tenant
    let (status, _, _) = ngsi_request(&http_client, Method::GET, &format!("/entities/{id_1}"), None, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    // 2. Batch Entity Creation
    let tenant_2 = format!("tenant-batch-create-{}", Uuid::new_v4());
    let id_2 = unique_entity_id("urn:ngsi-ld:Vehicle:batch-create");
    let batch = json!([vehicle_entity(&id_2, "v2")]);
    let (status, headers, _) = ngsi_request(&http_client, Method::POST, "/entityOperations/create", Some(&batch), &tenant_2, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant_2.as_str()));
    assert_tenant_serves(&http_client, &format!("/entities/{id_2}"), &tenant_2).await;
    let (status, _, _) = ngsi_request(&http_client, Method::GET, &format!("/entities/{id_2}"), None, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    // 3. Create Temporal Entity
    let tenant_3 = format!("tenant-temporal-create-{}", Uuid::new_v4());
    let id_3 = unique_entity_id("urn:ngsi-ld:Vehicle:temporal-create");
    let (status, headers, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/temporal/entities",
        Some(&temporal_vehicle_entity(&id_3)),
        &tenant_3,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant_3.as_str()));
    assert_tenant_serves(&http_client, &format!("/temporal/entities/{id_3}"), &tenant_3).await;
    let (status, _, _) = ngsi_request(&http_client, Method::GET, &format!("/temporal/entities/{id_3}"), None, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    // 4. Create Subscription
    let tenant_4 = format!("tenant-sub-create-{}", Uuid::new_v4());
    let id_4 = unique_subscription_id("sub-create");
    let (status, headers, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/subscriptions",
        Some(&entity_subscription(&id_4, "http://127.0.0.1:65535/notify")),
        &tenant_4,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant_4.as_str()));
    assert_tenant_serves(&http_client, &format!("/subscriptions/{id_4}"), &tenant_4).await;
    let (status, _, _) = ngsi_request(&http_client, Method::GET, &format!("/subscriptions/{id_4}"), None, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    // 5. Register Context Source
    let tenant_5 = format!("tenant-csr-create-{}", Uuid::new_v4());
    let id_5 = unique_entity_id("urn:ngsi-ld:ContextSourceRegistration:create");
    let (status, headers, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/csourceRegistrations",
        Some(&csr_registration(&id_5)),
        &tenant_5,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant_5.as_str()));
    assert_tenant_serves(&http_client, &format!("/csourceRegistrations/{id_5}"), &tenant_5).await;
    let (status, _, _) = ngsi_request(&http_client, Method::GET, &format!("/csourceRegistrations/{id_5}"), None, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    // 6. Create Context Source Subscription
    let tenant_6 = format!("tenant-csr-sub-create-{}", Uuid::new_v4());
    let id_6 = unique_subscription_id("csr-sub-create");
    let (status, headers, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/csourceSubscriptions",
        Some(&csr_subscription(&id_6, "http://127.0.0.1:65535/notify")),
        &tenant_6,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant_6.as_str()));
    assert_tenant_serves(&http_client, &format!("/csourceSubscriptions/{id_6}"), &tenant_6).await;
    let (status, _, _) = ngsi_request(&http_client, Method::GET, &format!("/csourceSubscriptions/{id_6}"), None, "", true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    environment.finish().await;
}

#[serial_test::serial]
#[tokio::test]
async fn multitenancy_named_tenant_error_response_echoes_ngsild_tenant_header() {
    let http_client = client();
    let environment = reset_broker_state().await;

    let tenant = format!("tenant-error-echo-{}", Uuid::new_v4());
    // Missing required `id` field → BadRequestData, but tenant header must still be echoed
    let invalid = json!({"type": "Vehicle", "@context": [CORE_CONTEXT]});
    let (status, resp_headers, _) = ngsi_request(&http_client, Method::POST, "/entities", Some(&invalid), &tenant, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(
        resp_headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()),
        Some(tenant.as_str()),
        "error responses in named tenant must echo NGSILD-Tenant header"
    );

    environment.finish().await;
}

#[serial_test::serial]
#[tokio::test]
async fn multitenancy_cross_tenant_entity_access_returns_not_found() {
    let http_client = client();
    let environment = reset_broker_state().await;

    let id = unique_entity_id("urn:ngsi-ld:Vehicle:cross-visibility");
    let tenant_a = format!("tenant-a-{}", Uuid::new_v4());
    let tenant_b = format!("tenant-b-{}", Uuid::new_v4());

    let (status, _, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/entities",
        Some(&vehicle_entity(&id, "tenant-a")),
        &tenant_a,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Accessing the entity from tenant B returns 404 NonexistentTenant (tenant B does not exist yet)
    let (status, _, body) = ngsi_request(&http_client, Method::GET, &format!("/entities/{id}"), None, &tenant_b, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(
        body["type"],
        NONEXISTENT_TENANT_ERROR.as_str(),
        "cross-tenant access to entity in non-existent tenant B must return NonexistentTenant"
    );

    environment.finish().await;
}

#[serial_test::serial]
#[tokio::test]
async fn multitenancy_unnamed_default_tenant_notifications_omit_tenant_header() {
    let http_client = client();
    let environment = reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let subscription_id = unique_subscription_id("unnamed-notif");
    let sub = entity_subscription(&subscription_id, &receiver.endpoint_url());
    let (status, resp_headers, _) = ngsi_request(&http_client, Method::POST, "/subscriptions", Some(&sub), "", false).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    assert!(
        resp_headers.get("NGSILD-Tenant").is_none(),
        "create subscription in unnamed default tenant must omit NGSILD-Tenant header"
    );

    // Small delay to let subscription activate before creating entity
    sleep(Duration::from_millis(150)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Vehicle:unnamed-notif");
    let (status, _, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/entities",
        Some(&vehicle_entity(&entity_id, "unnamed-trigger")),
        "",
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(10)).await;
    assert!(
        notification.headers.get("NGSILD-Tenant").is_none(),
        "notifications for unnamed default tenant must omit NGSILD-Tenant header"
    );

    receiver.shutdown().await;
    environment.finish().await;
}

#[serial_test::serial]
#[tokio::test]
async fn multitenancy_named_tenant_notifications_include_ngsild_tenant_header() {
    let http_client = client();
    let environment = reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let tenant = format!("tenant-notif-{}", Uuid::new_v4());

    let subscription_id = unique_subscription_id("named-notif");
    let sub = entity_subscription(&subscription_id, &receiver.endpoint_url());
    let (status, resp_headers, _) = ngsi_request(&http_client, Method::POST, "/subscriptions", Some(&sub), &tenant, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(
        resp_headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()),
        Some(tenant.as_str()),
        "create subscription in named tenant must include NGSILD-Tenant header"
    );

    sleep(Duration::from_millis(150)).await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Vehicle:named-notif");
    let (status, _, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/entities",
        Some(&vehicle_entity(&entity_id, "named-trigger")),
        &tenant,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(10)).await;
    pretty_assertions::assert_eq!(
        notification.headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()),
        Some(tenant.as_str()),
        "notifications for named tenant must include NGSILD-Tenant header"
    );

    receiver.shutdown().await;
    environment.finish().await;
}

#[serial_test::serial]
#[tokio::test]
async fn multitenancy_subscription_does_not_receive_events_from_different_tenant() {
    let http_client = client();
    let environment = reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let tenant_a = format!("tenant-sub-isolation-a-{}", Uuid::new_v4());
    let tenant_b = format!("tenant-sub-isolation-b-{}", Uuid::new_v4());

    // Subscription in tenant A
    let subscription_id = unique_subscription_id("isolation");
    let sub = entity_subscription(&subscription_id, &receiver.endpoint_url());
    let (status, _, _) = ngsi_request(&http_client, Method::POST, "/subscriptions", Some(&sub), &tenant_a, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    sleep(Duration::from_millis(150)).await;

    // Create entity in tenant B — must NOT trigger subscription in tenant A
    let entity_id = unique_entity_id("urn:ngsi-ld:Vehicle:sub-isolation");
    let (status, _, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/entities",
        Some(&vehicle_entity(&entity_id, "tenant-b-entity")),
        &tenant_b,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create entity in tenant A -- must trigger the subscription in tenant A. Its
    // notification is the sentinel that proves tenant B's create above stayed in
    // tenant B: delivery for one subscription is ordered on the shared endpoint,
    // so a cross-tenant leak would have arrived first and been the one returned
    // here. Asserting the delivered entity is tenant A's own is what detects a
    // leak -- a leaked tenant-B notification carries the same subscriptionId.
    let entity_id_a = unique_entity_id("urn:ngsi-ld:Vehicle:sub-isolation-a");
    let (status, _, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/entities",
        Some(&vehicle_entity(&entity_id_a, "tenant-a-entity")),
        &tenant_a,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    pretty_assertions::assert_eq!(notification.body["data"][0]["id"], entity_id_a);

    receiver.shutdown().await;
    environment.finish().await;
}

#[serial_test::serial]
#[tokio::test]
async fn multitenancy_tenants_endpoint_returns_a_tenant_array() {
    let http_client = client();
    let environment = reset_broker_state().await;

    let tenant_a = format!("tenant-list-a-{}", Uuid::new_v4());
    let tenant_b = format!("tenant-list-b-{}", Uuid::new_v4());

    let (status, _, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/entities",
        Some(&vehicle_entity(&unique_entity_id("urn:ngsi-ld:Vehicle:tenant-list-a"), "tenant-a-entity")),
        &tenant_a,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, _) = ngsi_request(
        &http_client,
        Method::POST,
        "/entities",
        Some(&vehicle_entity(&unique_entity_id("urn:ngsi-ld:Vehicle:tenant-list-b"), "tenant-b-entity")),
        &tenant_b,
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = list_tenants(&http_client).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    // The collection is now an array of `Tenant` resources, not the retired
    // `TenantList` wrapper object.
    let tenants = body.as_array().cloned().unwrap_or_default();
    let tenant_names: Vec<&str> = tenants.iter().filter_map(|tenant| tenant["tenantName"].as_str()).collect();
    assert!(
        tenant_names.contains(&tenant_a.as_str()),
        "the tenant array must contain {tenant_a}, got: {tenant_names:?}"
    );
    assert!(
        tenant_names.contains(&tenant_b.as_str()),
        "the tenant array must contain {tenant_b}, got: {tenant_names:?}"
    );

    // Every element carries the fixed `Tenant` type and a `urn:ngsi-ld:Tenant:`
    // identifier.
    for tenant in &tenants {
        pretty_assertions::assert_eq!(tenant["type"], "Tenant");
        let id = tenant["id"].as_str().unwrap_or_default();
        assert!(id.starts_with("urn:ngsi-ld:Tenant:"), "expected a Tenant URN id, got: {id}");
    }

    environment.finish().await;
}
