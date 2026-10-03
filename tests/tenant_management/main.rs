//! Tenant-management extension integration tests (ETSI GS CIM 009 v1.9.1 leaves
//! tenant management implementation-specific, clause 5.5.10).
//!
//! Runs against a base-tier broker with `ANDROMEDA__EXTENSIONS__TENANT_MANAGEMENT=enabled`.
//! Covers the suspend/resume lifecycle, the 404 a suspended tenant answers on the
//! data path, the `status` residency values, the `options` blocks, the paginated
//! collection, tenant isolation, and that the retired `evictOrphans` route is
//! gone.
//!
//! Naming convention: `tenant_management_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::{context::CORE_CONTEXT, problem_detail as error};
use andromeda_test_harness::{
    configuration::RUNTIME_BROKER_BASE as BASE_URL,
    database_access::connect_test_database,
    serialized_environment::SerializedEnvironment,
};
use andromeda_test_mocks::{
    callback_path::CallbackPath,
    callback_server::{CallbackServer, MockResponse, RequestMatcher, ResponseGate},
    server_config::MockServerConfig,
};
use bytes::Bytes;
use reqwest::{
    Client,
    Method,
    StatusCode,
    header::{CONTENT_TYPE, HeaderMap, HeaderValue},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

const NONEXISTENT_TENANT_ERROR: &iri_rs::Iri<&str> = error::NONEXISTENT_TENANT;

fn client() -> Client {
    Client::new()
}

fn link_header() -> String {
    format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

fn unique_tenant(prefix: &str) -> String {
    format!("{prefix}{}", Uuid::new_v4().simple())
}

/// A minimal `Device` entity under the core context.
fn device_entity(id: &str) -> Value {
    json!({"id": id, "type": "Device", "reading": {"type": "Property", "value": 1}, "@context": [CORE_CONTEXT]})
}

/// Create one entity under `tenant`, returning the response status and the echoed
/// `NGSILD-Tenant` header.
async fn create_entity(http: &Client, tenant: &str, id: &str) -> (StatusCode, Option<String>) {
    let response = http
        .post(format!("{BASE_URL}/entities"))
        .header("NGSILD-Tenant", tenant)
        .header("Content-Type", "application/ld+json")
        .json(&device_entity(id))
        .send()
        .await
        .expect("create request");
    let status = response.status();
    let echoed = response.headers().get("NGSILD-Tenant").and_then(|value| value.to_str().ok()).map(str::to_owned);
    (status, echoed)
}

/// Retrieve one entity under `tenant`, returning the status, the echoed tenant
/// header, and the body.
async fn retrieve_entity(http: &Client, tenant: &str, id: &str) -> (StatusCode, Option<String>, Value) {
    let encoded = urlencoding::encode(id);
    let response = http
        .get(format!("{BASE_URL}/entities/{encoded}"))
        .header("NGSILD-Tenant", tenant)
        .header("Accept", "application/ld+json")
        .header("Link", link_header())
        .send()
        .await
        .expect("retrieve request");
    let status = response.status();
    let echoed = response.headers().get("NGSILD-Tenant").and_then(|value| value.to_str().ok()).map(str::to_owned);
    let body = response.json::<Value>().await.unwrap_or(Value::Null);
    (status, echoed, body)
}

/// `GET /tenants/{tenantName}` with optional `options`, returning status and body.
async fn get_tenant(http: &Client, tenant: &str, options: Option<&str>) -> (StatusCode, Value) {
    let url = match options {
        Some(options) => format!("{BASE_URL}/tenants/{tenant}?options={options}"),
        None => format!("{BASE_URL}/tenants/{tenant}"),
    };
    let response = http.get(url).send().await.expect("get tenant request");
    let status = response.status();
    let body = response.json::<Value>().await.unwrap_or(Value::Null);
    (status, body)
}

/// `PATCH /tenants/{tenantName}` setting `isActive`, returning the status.
async fn patch_active(http: &Client, tenant: &str, is_active: bool) -> StatusCode {
    http.patch(format!("{BASE_URL}/tenants/{tenant}"))
        .header("Content-Type", "application/json")
        .body(json!({"isActive": is_active}).to_string())
        .send()
        .await
        .expect("patch request")
        .status()
}

/// `GET /tenants` with an optional query, returning status, headers and body.
async fn list_tenants(http: &Client, query: &str) -> (StatusCode, HeaderMap, Value) {
    let response = http.get(format!("{BASE_URL}/tenants{query}")).send().await.expect("list request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json::<Value>().await.unwrap_or(Value::Null);
    (status, headers, body)
}

/// The `cursor` query the `rel="next"` `Link` points at, if any.
fn next_cursor(headers: &HeaderMap) -> Option<String> {
    let link = headers.get("Link")?.to_str().ok()?;
    let next = link.split(',').find(|part| part.contains("rel=\"next\""))?;
    let target = next.split('<').nth(1)?.split('>').next()?;
    let query = target.split('?').nth(1)?;
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("cursor="))
        .map(|cursor| match urlencoding::decode(cursor) {
            Ok(decoded) => decoded.into_owned(),
            Err(_) => cursor.to_owned(),
        })
}

#[must_use = "finish the environment so tenant databases and their background workers are removed"]
struct Environment {
    serialized: SerializedEnvironment,
    pool: PgPool,
}

async fn reset() -> Environment {
    let mut serialized = SerializedEnvironment::from_environment_reset().await.expect("serialized environment");
    let pool = connect_test_database().await.expect("connect to test database");
    clean_tenants(&pool).await;
    drop_tenant_databases(&pool).await;
    serialized.reset().await.expect("serialized reset");
    Environment { serialized, pool }
}

impl Environment {
    async fn finish(mut self) {
        clean_tenants(&self.pool).await;
        drop_tenant_databases(&self.pool).await;
        self.serialized.reset().await.expect("final serialized reset");
        self.pool.close().await;
        self.serialized.finish().await.expect("serialized release");
    }
}

/// Delete every named-tenant registry row.
async fn clean_tenants(pool: &PgPool) {
    let has_table: bool = sqlx::query_scalar("SELECT to_regclass('public.tenants') IS NOT NULL")
        .fetch_one(pool)
        .await
        .unwrap_or(false);
    if has_table {
        let _ = sqlx::query("DELETE FROM tenants WHERE is_unnamed_default = false").execute(pool).await;
    }
}

/// Drop every `andromeda_tenant_%` database, forcing off residual backends.
async fn drop_tenant_databases(pool: &PgPool) {
    let current: String = sqlx::query_scalar("SELECT current_database()").fetch_one(pool).await.unwrap_or_default();
    let dbs: Vec<String> = sqlx::query_scalar("SELECT datname FROM pg_database WHERE datname LIKE 'andromeda_tenant_%' AND datname <> $1")
        .bind(&current)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    for db in dbs {
        let statement = format!("DROP DATABASE IF EXISTS \"{db}\" WITH (FORCE)");
        let _ = sqlx::query(sqlx::AssertSqlSafe(statement)).execute(pool).await;
    }
}

#[serial_test::serial]
#[tokio::test]
async fn tenant_management_suspend_resume_round_trips_through_a_404() {
    let environment = reset().await;
    let http = client();
    let tenant = unique_tenant("tmlife");
    let id = "urn:ngsi-ld:Device:tm-lifecycle";

    // Implicit creation on first write (clause 5.5.10).
    let (status, _) = create_entity(&http, &tenant, id).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // The tenant appears in the collection and reads as in service.
    let (status, _, body) = list_tenants(&http, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = body.as_array().into_iter().flatten().filter_map(|value| value["tenantName"].as_str()).collect();
    assert!(names.contains(&tenant.as_str()), "the collection must list {tenant}, got {names:?}");

    let (status, body) = get_tenant(&http, &tenant, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["isActive"], json!(true));
    let live = body["status"].as_str().unwrap_or_default();
    assert!(
        live == "active" || live == "dormant",
        "an in-service tenant reads active or dormant, got {live}"
    );

    // Suspend: the data path now answers 404 NonexistentTenant, echoing the tenant.
    pretty_assertions::assert_eq!(patch_active(&http, &tenant, false).await, StatusCode::NO_CONTENT);
    let (status, echoed, body) = retrieve_entity(&http, &tenant, id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"].as_str(), Some(NONEXISTENT_TENANT_ERROR.as_str()));
    pretty_assertions::assert_eq!(echoed.as_deref(), Some(tenant.as_str()), "the NGSILD-Tenant header is echoed (clause 6.3.14)");

    // The management path still reports the real status.
    let (status, body) = get_tenant(&http, &tenant, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["isActive"], json!(false));
    pretty_assertions::assert_eq!(body["status"], json!("suspended"));

    // Resume: the data written before the suspension is intact (the DB was never dropped).
    pretty_assertions::assert_eq!(patch_active(&http, &tenant, true).await, StatusCode::NO_CONTENT);
    let (status, _, body) = retrieve_entity(&http, &tenant, id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(
        body["id"].as_str().is_some_and(|value| value.contains("tm-lifecycle")),
        "the entity survives suspend/resume, got {body}"
    );

    environment.finish().await;
}

#[serial_test::serial]
#[tokio::test]
async fn tenant_management_options_add_sys_attrs_and_usage() {
    let environment = reset().await;
    let http = client();
    let tenant = unique_tenant("tmopts");

    // Two entities so entityCount is a nonzero, known value.
    for index in 0..2 {
        let (status, _) = create_entity(&http, &tenant, &format!("urn:ngsi-ld:Device:tm-opts-{index}")).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    // Default: no timestamps, no usage.
    let (status, body) = get_tenant(&http, &tenant, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("createdAt").is_none(), "default read omits createdAt, got {body}");
    assert!(body.get("databaseSize").is_none(), "default read omits usage, got {body}");

    // sysAttrs surfaces the timestamps.
    let (status, body) = get_tenant(&http, &tenant, Some("sysAttrs")).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body["createdAt"].as_str().is_some(), "sysAttrs adds createdAt, got {body}");
    assert!(body["modifiedAt"].as_str().is_some(), "sysAttrs adds modifiedAt, got {body}");

    // usage surfaces the operational block.
    let (status, body) = get_tenant(&http, &tenant, Some("usage")).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(
        body["databaseSize"].as_u64().unwrap_or(0) > 0,
        "usage reports a nonzero databaseSize, got {body}"
    );
    pretty_assertions::assert_eq!(body["entityCount"], json!(2));
    assert!(body.get("snapshotCount").is_some(), "usage reports a snapshotCount, got {body}");
    assert!(body.get("poolConnections").is_some(), "usage reports poolConnections, got {body}");

    // An unknown options keyword is refused (clause 6.3.20).
    let (status, _) = get_tenant(&http, &tenant, Some("bogus")).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);

    environment.finish().await;
}

#[serial_test::serial]
#[tokio::test]
async fn tenant_management_collection_paginates_by_cursor() {
    let environment = reset().await;
    let http = client();

    let mut created: Vec<String> = Vec::new();
    for index in 0..5 {
        let tenant = unique_tenant(&format!("tmpage{index}"));
        let (status, _) = create_entity(&http, &tenant, &format!("urn:ngsi-ld:Device:tm-page-{index}")).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
        created.push(tenant);
    }

    // Walk the collection two at a time, following the next cursor.
    let mut seen: Vec<String> = Vec::new();
    let mut query = "?limit=2".to_owned();
    // The loop body runs before any break, so this is always assigned before it
    // is read after the walk.
    let mut total_header: Option<usize>;
    loop {
        let (status, headers, body) = list_tenants(&http, &query).await;
        pretty_assertions::assert_eq!(status, StatusCode::OK);
        total_header = headers
            .get("NGSILD-Results-Count")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok());
        for tenant in body.as_array().into_iter().flatten() {
            if let Some(name) = tenant["tenantName"].as_str() {
                seen.push(name.to_owned());
            }
        }
        match next_cursor(&headers) {
            Some(cursor) => query = format!("?limit=2&cursor={}", urlencoding::encode(&cursor)),
            None => break,
        }
    }

    for tenant in &created {
        assert!(seen.contains(tenant), "walking next must visit {tenant}, saw {seen:?}");
    }
    // Each tenant is visited exactly once.
    let mut unique = seen.clone();
    unique.sort();
    unique.dedup();
    pretty_assertions::assert_eq!(unique.len(), seen.len(), "each tenant is visited once, saw {seen:?}");
    pretty_assertions::assert_eq!(total_header, Some(created.len()), "the count header matches the total");

    environment.finish().await;
}

/// Clause 5.5.9.2: the tenant collection pages by `limit`/`offset` in tenant
/// name order beside its cursor walk, with offset pointers back.
#[serial_test::serial]
#[tokio::test]
async fn tenant_management_collection_paginates_by_offset() {
    let environment = reset().await;
    let http = client();

    let mut created: Vec<String> = Vec::new();
    for index in 0..3 {
        let tenant = unique_tenant(&format!("tmoffset{index}"));
        let (status, _) = create_entity(&http, &tenant, &format!("urn:ngsi-ld:Device:tm-offset-{index}")).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
        created.push(tenant);
    }
    created.sort();
    let created = created;

    let (status, headers, body) = list_tenants(&http, "?offset=1&limit=1").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = body
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tenant| tenant["tenantName"].as_str())
        .collect();
    pretty_assertions::assert_eq!(names, vec![created[1].as_str()], "offset 1 is the second tenant by name");
    let link = headers.get("Link").and_then(|value| value.to_str().ok()).unwrap_or_default();
    assert!(link.contains("offset=0"), "the previous page is named by offset: {link}");
    assert!(link.contains("offset=2"), "the next page is named by offset: {link}");
    assert!(!link.contains("cursor="), "an offset page carries no cursor: {link}");

    environment.finish().await;
}

#[serial_test::serial]
#[tokio::test]
async fn tenant_management_evict_orphans_route_is_retired() {
    let environment = reset().await;
    let http = client();
    // `POST /tenants/evictOrphans` now falls under the `/tenants/{tenantName}`
    // resource (tenant name `evictOrphans`), which serves only GET and PATCH, so
    // the retired POST reaches no handler and is answered 405.
    let response = http.post(format!("{BASE_URL}/tenants/evictOrphans")).send().await.expect("request");
    pretty_assertions::assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    environment.finish().await;
}

#[serial_test::serial]
#[tokio::test]
async fn tenant_management_suspending_one_tenant_leaves_another_serving() {
    let environment = reset().await;
    let http = client();
    let acme = unique_tenant("tmiso-a");
    let beta = unique_tenant("tmiso-b");
    let acme_id = "urn:ngsi-ld:Device:tm-iso-a";
    let beta_id = "urn:ngsi-ld:Device:tm-iso-b";

    pretty_assertions::assert_eq!(create_entity(&http, &acme, acme_id).await.0, StatusCode::CREATED);
    pretty_assertions::assert_eq!(create_entity(&http, &beta, beta_id).await.0, StatusCode::CREATED);

    // Suspend acme.
    pretty_assertions::assert_eq!(patch_active(&http, &acme, false).await, StatusCode::NO_CONTENT);
    pretty_assertions::assert_eq!(retrieve_entity(&http, &acme, acme_id).await.0, StatusCode::NOT_FOUND);

    // beta is unaffected: it reads its data and accepts a new write.
    pretty_assertions::assert_eq!(retrieve_entity(&http, &beta, beta_id).await.0, StatusCode::OK);
    let (status, _) = create_entity(&http, &beta, "urn:ngsi-ld:Device:tm-iso-b2").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    environment.finish().await;
}

/// Clause 5.5.10 with the suspension contract of `docs/tenant-management.md`:
/// a suspended tenant answers every data-path request `404 NonexistentTenant`,
/// even while a request admitted before the suspension still holds the
/// tenant's runtime. That in-flight request once kept the runtime cached and
/// serving after the suspension, because eviction spared a runtime in use and
/// nothing evicted it when the request finished.
///
/// The in-flight request is held open deterministically: it is a retrieval
/// forwarded to a registered Context Source whose answer waits at a closed
/// gate until the test releases it.
#[serial_test::serial]
#[tokio::test]
async fn tenant_management_suspension_refuses_the_tenant_while_a_request_still_holds_it() {
    let environment = reset().await;
    let http = client();
    let tenant = unique_tenant("tmleased");
    let local_id = "urn:ngsi-ld:Device:tm-leased-local";
    let remote_id = "urn:ngsi-ld:Device:tm-leased-remote";
    let (status, _) = create_entity(&http, &tenant, local_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let source = CallbackServer::start(MockServerConfig::from_environment().expect("context source configuration"))
        .await
        .expect("gated context source starts");
    let gate = ResponseGate::closed();
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"));
    let retrieval_path = CallbackPath::parse(format!("/ngsi-ld/v1/entities/{remote_id}")).expect("retrieval path");
    source
        .enqueue(
            RequestMatcher::new(Method::GET, retrieval_path),
            MockResponse::builder()
                .status(StatusCode::OK)
                .headers(headers)
                .body(Bytes::from(device_entity(remote_id).to_string()))
                .gate(Some(gate.clone()))
                .build(),
        )
        .await;
    let endpoint = source.endpoint(&CallbackPath::parse("/").expect("root path")).expect("context source endpoint");
    let registration = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:tm-leased",
        "type": "ContextSourceRegistration",
        "endpoint": endpoint,
        "information": [{"entities": [{"id": remote_id, "type": "Device"}]}],
        "@context": [CORE_CONTEXT]
    });
    let registered = http
        .post(format!("{BASE_URL}/csourceRegistrations"))
        .header("NGSILD-Tenant", &tenant)
        .header("Content-Type", "application/ld+json")
        .json(&registration)
        .send()
        .await
        .expect("registration request");
    pretty_assertions::assert_eq!(registered.status(), StatusCode::CREATED);

    let in_flight = tokio::spawn({
        let http = http.clone(); // clone: the spawned request owns its handle to the shared client
        let tenant = tenant.clone(); // clone: the spawned request owns the tenant it addresses
        async move { retrieve_entity(&http, &tenant, remote_id).await.0 }
    });
    source
        .next_request(Duration::from_secs(10))
        .await
        .expect("the retrieval reaches the gated source and holds the tenant's runtime");

    pretty_assertions::assert_eq!(patch_active(&http, &tenant, false).await, StatusCode::NO_CONTENT);
    let (status, _, body) = retrieve_entity(&http, &tenant, local_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND, "a suspended tenant is refused while a request still holds it");
    pretty_assertions::assert_eq!(body["type"].as_str(), Some(NONEXISTENT_TENANT_ERROR.as_str()));

    gate.release();
    in_flight.await.expect("the in-flight retrieval finishes");
    let (status, _, body) = retrieve_entity(&http, &tenant, local_id).await;
    pretty_assertions::assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a suspended tenant stays refused after the last request releases it"
    );
    pretty_assertions::assert_eq!(body["type"].as_str(), Some(NONEXISTENT_TENANT_ERROR.as_str()));

    pretty_assertions::assert_eq!(patch_active(&http, &tenant, true).await, StatusCode::NO_CONTENT);
    let (status, _, _) = retrieve_entity(&http, &tenant, local_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK, "a resumed tenant serves again");

    source.shutdown().await.expect("context source shutdown");
    environment.finish().await;
}
