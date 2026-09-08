//! Snapshot tenancy, replica durability, and worker-resilience acceptance cases.

use super::*;
use andromeda_test_harness::database_reset::reset_owned_tenant;
use andromeda_test_mocks::{
    callback_path::CallbackPath,
    callback_server::{CallbackServer, MockResponse, RequestMatcher, ResponseGate},
    server_config::MockServerConfig,
};
use axum::{
    body::Bytes,
    http::{HeaderMap, Method, StatusCode as AxumStatusCode, header::CONTENT_TYPE},
};
use futures::future::join_all;
use sqlx::PgPool;
use std::env;
use tokio::{task::yield_now, time::timeout};

const REPLICA_BASE: &str = "http://127.0.0.1:1082/ngsi-ld/v1";
const NON_MULTITENANT_BASE: &str = "http://127.0.0.1:1084/ngsi-ld/v1";
const QUOTA_BASE: &str = "http://127.0.0.1:1085/ngsi-ld/v1";
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

async fn create_for_tenant(base: &str, tenant: Option<&str>, id: &str, lifetime: &str) -> StatusCode {
    let mut request = client()
        .post(format!("{base}/snapshots"))
        .header("Content-Type", "application/json")
        .json(&json!({"id": id, "type": "Snapshot", "snapshotQueries": [], "snapshotLifetime": lifetime}));
    if let Some(tenant) = tenant {
        request = request.header("NGSILD-Tenant", tenant);
    }
    request.send().await.expect("tenant Snapshot create").status()
}

async fn get_for_tenant(base: &str, tenant: Option<&str>, id: &str) -> StatusCode {
    let mut request = client().get(format!("{base}/snapshots/{id}"));
    if let Some(tenant) = tenant {
        request = request.header("NGSILD-Tenant", tenant);
    }
    request.send().await.expect("tenant Snapshot get").status()
}

async fn provision_tenant(base: &str, tenant: &str) {
    let entity_id = format!("urn:ngsi-ld:Vehicle:provision-{tenant}");
    let response = client()
        .post(format!("{base}/entities"))
        .header("Content-Type", "application/ld+json")
        .header("NGSILD-Tenant", tenant)
        .json(&json!({"id": entity_id, "type": "Vehicle", "@context": CORE_CONTEXT}))
        .send()
        .await
        .expect("tenant provisioning Entity create");
    pretty_assertions::assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_default_tenant_applies_when_tenant_header_is_absent() {
    let _serialized_environment = clean_database().await;
    let id = "urn:ngsi-ld:Snapshot:default-tenant";
    pretty_assertions::assert_eq!(create_for_tenant(&broker_url(""), None, id, "PT1H").await, StatusCode::CREATED);
    pretty_assertions::assert_eq!(get_for_tenant(&broker_url(""), None, id).await, StatusCode::OK);
    pretty_assertions::assert_eq!(get_for_tenant(&broker_url(""), Some("named-default-check"), id).await, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_tenant_isolation_allows_same_ids_and_requires_both_headers() {
    let _serialized_environment = clean_database().await;
    let id = "urn:ngsi-ld:Snapshot:same-across-tenants";
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let alpha = format!("snapshot-alpha-{suffix}");
    let beta = format!("snapshot-beta-{suffix}");
    for tenant in [&alpha, &beta] {
        provision_tenant(&broker_url(""), tenant).await;
        pretty_assertions::assert_eq!(create_for_tenant(&broker_url(""), Some(tenant), id, "PT1H").await, StatusCode::CREATED);
    }
    pretty_assertions::assert_eq!(get_for_tenant(&broker_url(""), Some(&alpha), id).await, StatusCode::OK);
    pretty_assertions::assert_eq!(get_for_tenant(&broker_url(""), Some(&beta), id).await, StatusCode::OK);
    pretty_assertions::assert_eq!(get_for_tenant(&broker_url(""), None, id).await, StatusCode::NOT_FOUND);
    let wrong_tenant_scope = client()
        .get(broker_url("/entities"))
        .header("NGSILD-Tenant", "snapshot-gamma")
        .header("NGSILD-Snapshot", id)
        .send()
        .await
        .expect("wrong tenant scoped request");
    pretty_assertions::assert_eq!(wrong_tenant_scope.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_missing_tenant_is_not_created_implicitly() {
    let _serialized_environment = clean_database().await;
    let pool = PgPool::connect(&env::var("ANDROMEDA_TEST_DATABASE_URL").expect("Snapshot database URL"))
        .await
        .expect("default database connection");
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tenants WHERE name = 'snapshot-missing'")
        .fetch_one(&pool)
        .await
        .expect("tenant count before scope");
    let response = client()
        .get(broker_url("/entities"))
        .header("NGSILD-Tenant", "snapshot-missing")
        .header("NGSILD-Snapshot", "urn:ngsi-ld:Snapshot:absent")
        .send()
        .await
        .expect("missing tenant scoped request");
    pretty_assertions::assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tenants WHERE name = 'snapshot-missing'")
        .fetch_one(&pool)
        .await
        .expect("tenant count after scope");
    pretty_assertions::assert_eq!((before, after), (0, 0));
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_non_multitenant_broker_rejects_explicit_tenant() {
    let _serialized_environment = clean_database().await;
    let response = client()
        .post(format!("{NON_MULTITENANT_BASE}/snapshots"))
        .header("Content-Type", "application/json")
        .header("NGSILD-Tenant", "forbidden")
        .json(&json!({"type": "Snapshot", "snapshotQueries": []}))
        .send()
        .await
        .expect("non-multitenant request");
    pretty_assertions::assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let problem: Value = response.json().await.expect("tenancy ProblemDetails");
    assert!(problem["detail"].as_str().is_some_and(|detail| detail.contains("not enabled")));
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_replica_reads_and_mutates_shared_branch_durably() {
    let _serialized_environment = clean_database().await;
    let id = "urn:ngsi-ld:Snapshot:replica-durable";
    pretty_assertions::assert_eq!(create_for_tenant(&broker_url(""), None, id, "PT1H").await, StatusCode::CREATED);
    let scoped_entity = json!({"id": "urn:ngsi-ld:Vehicle:replica", "type": "Vehicle", "speed": {"type": "Property", "value": 41}, "@context": CORE_CONTEXT});
    let create = client()
        .post(format!("{REPLICA_BASE}/entities"))
        .header("Content-Type", "application/ld+json")
        .header("NGSILD-Snapshot", id)
        .json(&scoped_entity)
        .send()
        .await
        .expect("replica scoped entity create");
    pretty_assertions::assert_eq!(create.status(), StatusCode::CREATED);
    let primary_read = client()
        .get(broker_url("/entities/urn:ngsi-ld:Vehicle:replica"))
        .header("NGSILD-Snapshot", id)
        .send()
        .await
        .expect("primary scoped entity read");
    pretty_assertions::assert_eq!(primary_read.status(), StatusCode::OK);
    pretty_assertions::assert_eq!(primary_read.json::<Value>().await.expect("entity body")["speed"]["value"], 41);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_expiry_cascades_branch_and_is_tenant_local() {
    let _serialized_environment = clean_database().await;
    let id = "urn:ngsi-ld:Snapshot:tenant-expiry";
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let expires = format!("expires-{suffix}");
    let survives = format!("survives-{suffix}");
    provision_tenant(&broker_url(""), &expires).await;
    provision_tenant(&broker_url(""), &survives).await;
    pretty_assertions::assert_eq!(create_for_tenant(&broker_url(""), Some(&expires), id, "PT1S").await, StatusCode::CREATED);
    pretty_assertions::assert_eq!(create_for_tenant(&broker_url(""), Some(&survives), id, "PT1H").await, StatusCode::CREATED);
    timeout(Duration::from_secs(8), async {
        loop {
            if get_for_tenant(&broker_url(""), Some(&expires), id).await == StatusCode::NOT_FOUND {
                break;
            }
            yield_now().await;
        }
    })
    .await
    .expect("short-lived tenant Snapshot expires");
    pretty_assertions::assert_eq!(get_for_tenant(&broker_url(""), Some(&survives), id).await, StatusCode::OK);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_concurrent_preparation_claim_executes_each_query_once() {
    let _serialized_environment = clean_database().await;
    let entity = json!({"id": "urn:ngsi-ld:Vehicle:claim-once", "type": "Vehicle", "@context": CORE_CONTEXT});
    pretty_assertions::assert_eq!(create_entity(&client(), &entity).await, StatusCode::CREATED);
    let id = "urn:ngsi-ld:Snapshot:claim-once";
    pretty_assertions::assert_eq!(
        create_snapshot_raw(
            &client(),
            &json!({"id": id, "type": "Snapshot", "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]})
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let requests = [broker_url("/entities?type=Vehicle"), format!("{REPLICA_BASE}/entities?type=Vehicle")]
        .into_iter()
        .map(|url| async move {
            client()
                .get(url)
                .header("NGSILD-Snapshot", id)
                .send()
                .await
                .expect("concurrent scoped query")
                .status()
        });
    for status in join_all(requests).await {
        pretty_assertions::assert_eq!(status, StatusCode::OK);
    }
    let terminal = wait_for_snapshot_terminal(&client(), id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(terminal["snapshotQueriesDetails"].as_array().expect("query details").len(), 1);
    pretty_assertions::assert_eq!(
        query_snapshot_entities(&client(), id, "type=Vehicle")
            .await
            .2
            .as_array()
            .expect("entity result")
            .len(),
        1
    );
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_status_notification_retries_without_rolling_back_committed_state() {
    let _serialized_environment = clean_database().await;
    let callback = CallbackServer::start(MockServerConfig::from_environment().expect("callback configuration"))
        .await
        .expect("callback server starts");
    let path = CallbackPath::parse("/snapshot-retry").expect("static callback path");
    let matcher = RequestMatcher::new(Method::POST, path.clone());
    callback
        .enqueue(
            matcher.clone(),
            MockResponse::builder()
                .status(AxumStatusCode::SERVICE_UNAVAILABLE)
                .headers(HeaderMap::new())
                .body(Bytes::new())
                .build(),
        )
        .await;
    callback
        .enqueue(
            matcher,
            MockResponse::builder()
                .status(AxumStatusCode::NO_CONTENT)
                .headers(HeaderMap::new())
                .body(Bytes::new())
                .build(),
        )
        .await;
    let id = "urn:ngsi-ld:Snapshot:status-retry";
    let endpoint = callback.endpoint(&path).expect("callback endpoint");
    pretty_assertions::assert_eq!(
        create_snapshot_raw(
            &client(),
            &json!({"id": id, "type": "Snapshot", "snapshotQueries": [], "endpoint": endpoint, "receiverInfo": [{"key": "X-Retry-Proof", "value": "yes"}]})
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let first = callback.next_request(Duration::from_secs(10)).await.expect("first status attempt");
    pretty_assertions::assert_eq!(first.headers["content-type"], "application/ld+json");
    pretty_assertions::assert_eq!(first.headers["X-Retry-Proof"], "yes");
    pretty_assertions::assert_eq!(first.json::<Value>().expect("first notification JSON")["snapshotId"], id);
    pretty_assertions::assert_eq!(
        wait_for_snapshot_terminal(&client(), id, Duration::from_secs(5)).await["snapshotStatus"],
        "failure"
    );
    let second = callback.next_request(Duration::from_secs(12)).await.expect("retried status attempt");
    pretty_assertions::assert_eq!(second.json::<Value>().expect("second notification JSON")["snapshotId"], id);
    pretty_assertions::assert_eq!(get_snapshot(&client(), id).await.0, StatusCode::OK);
    callback.shutdown().await.expect("callback server shuts down");
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_resource_pressure_evicts_by_priority_expiry_last_use_and_stable_id() {
    let _serialized_environment = clean_database().await;
    let primary_url = env::var("ANDROMEDA_TEST_DATABASE_URL").expect("Snapshot database URL");
    let (server_url, _) = primary_url.rsplit_once('/').expect("database URL has a database path");
    let quota_pool = PgPool::connect(&format!("{server_url}/snapshot_quota"))
        .await
        .expect("quota database connection");
    reset_owned_tenant(&quota_pool).await.expect("quota database reset");
    let definitions = [
        ("urn:ngsi-ld:Snapshot:quota-low-priority", 1, "PT10H"),
        ("urn:ngsi-ld:Snapshot:quota-early-expiry", 5, "PT1H"),
        ("urn:ngsi-ld:Snapshot:quota-late-expiry", 5, "PT2H"),
        ("urn:ngsi-ld:Snapshot:quota-high-priority", 10, "PT1H"),
    ];
    for (id, priority, lifetime) in definitions {
        let response = client()
            .post(format!("{QUOTA_BASE}/snapshots"))
            .header("Content-Type", "application/json")
            .json(&json!({"id": id, "type": "Snapshot", "snapshotQueries": [], "snapshotPriority": priority, "snapshotLifetime": lifetime}))
            .send()
            .await
            .expect("quota Snapshot create");
        pretty_assertions::assert_eq!(response.status(), StatusCode::CREATED);
    }
    pretty_assertions::assert_eq!(get_for_tenant(QUOTA_BASE, None, definitions[0].0).await, StatusCode::NOT_FOUND);
    for (id, _, _) in &definitions[1..] {
        pretty_assertions::assert_eq!(get_for_tenant(QUOTA_BASE, None, id).await, StatusCode::OK);
    }
    let fifth = "urn:ngsi-ld:Snapshot:quota-fifth";
    pretty_assertions::assert_eq!(create_for_tenant(QUOTA_BASE, None, fifth, "PT3H").await, StatusCode::CREATED);
    pretty_assertions::assert_eq!(get_for_tenant(QUOTA_BASE, None, definitions[1].0).await, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(get_for_tenant(QUOTA_BASE, None, fifth).await, StatusCode::OK);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_create_commits_preparing_and_returns_before_capture() {
    let _serialized_environment = clean_database().await;
    let callback = CallbackServer::start(MockServerConfig::from_environment().expect("context source configuration"))
        .await
        .expect("gated context source starts");
    let path = CallbackPath::parse("/ngsi-ld/v1/entities").expect("static query path");
    let gate = ResponseGate::closed();
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, "application/ld+json".parse().expect("static content type"));
    callback
        .enqueue(
            RequestMatcher::new(Method::GET, path.clone()),
            MockResponse::builder()
                .status(AxumStatusCode::OK)
                .headers(headers)
                .body(Bytes::from_static(b"[]"))
                .gate(Some(gate.clone()))
                .build(),
        )
        .await;
    let endpoint = callback
        .endpoint(&CallbackPath::parse("/").expect("root path"))
        .expect("context source endpoint");
    let entity_type = "https://example.org/GatedCapture";
    let registration = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:gated-capture",
        "type": "ContextSourceRegistration",
        "endpoint": endpoint,
        "information": [{"entities": [{"type": entity_type}]}],
        "operations": ["queryEntity"],
        "@context": CORE_CONTEXT
    });
    pretty_assertions::assert_eq!(
        client()
            .post(broker_url("/csourceRegistrations"))
            .header("Content-Type", "application/ld+json")
            .json(&registration)
            .send()
            .await
            .expect("gated source registration")
            .status(),
        StatusCode::CREATED
    );
    let id = "urn:ngsi-ld:Snapshot:gated-capture";
    pretty_assertions::assert_eq!(
        create_snapshot_raw(
            &client(),
            &json!({"id": id, "type": "Snapshot", "snapshotQueries": [{"type": "Query", "entities": [{"type": entity_type}]}]})
        )
        .await
        .0,
        StatusCode::CREATED
    );
    callback.next_request(Duration::from_secs(5)).await.expect("capture reaches gated source");
    pretty_assertions::assert_eq!(get_snapshot(&client(), id).await.1["snapshotStatus"], "preparing");
    let mut scoped = tokio::spawn(async move { query_snapshot_entities(&client(), id, &format!("type={entity_type}")).await });
    assert!(
        timeout(Duration::from_millis(250), &mut scoped).await.is_err(),
        "scoped operation must wait during preparation"
    );
    gate.release();
    let released = scoped.await.expect("released scoped query task");
    pretty_assertions::assert_eq!(released.0, StatusCode::OK);
    pretty_assertions::assert_eq!(released.2, json!([]));
    pretty_assertions::assert_eq!(callback.total_request_count(), 1, "scoped branch query must not forward after capture");
    callback.shutdown().await.expect("gated context source shuts down");

    let timeout_callback = CallbackServer::start(MockServerConfig::from_environment().expect("timeout source configuration"))
        .await
        .expect("timeout context source starts");
    let timeout_gate = ResponseGate::closed();
    let mut timeout_headers = HeaderMap::new();
    timeout_headers.insert(CONTENT_TYPE, "application/ld+json".parse().expect("static content type"));
    timeout_callback
        .enqueue(
            RequestMatcher::new(Method::GET, path),
            MockResponse::builder()
                .status(AxumStatusCode::OK)
                .headers(timeout_headers)
                .body(Bytes::from_static(b"[]"))
                .gate(Some(timeout_gate.clone()))
                .build(),
        )
        .await;
    let timeout_endpoint = timeout_callback
        .endpoint(&CallbackPath::parse("/").expect("root path"))
        .expect("timeout source endpoint");
    let timeout_type = "https://example.org/TimeoutCapture";
    let timeout_registration = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:timeout-capture",
        "type": "ContextSourceRegistration",
        "endpoint": timeout_endpoint,
        "information": [{"entities": [{"type": timeout_type}]}],
        "operations": ["queryEntity"],
        "@context": CORE_CONTEXT
    });
    pretty_assertions::assert_eq!(
        client()
            .post(broker_url("/csourceRegistrations"))
            .header("Content-Type", "application/ld+json")
            .json(&timeout_registration)
            .send()
            .await
            .expect("timeout source registration")
            .status(),
        StatusCode::CREATED
    );
    let timeout_id = "urn:ngsi-ld:Snapshot:timeout-capture";
    pretty_assertions::assert_eq!(
        create_snapshot_raw(
            &client(),
            &json!({"id": timeout_id, "type": "Snapshot", "snapshotQueries": [{"type": "Query", "entities": [{"type": timeout_type}]}]})
        )
        .await
        .0,
        StatusCode::CREATED
    );
    timeout_callback
        .next_request(Duration::from_secs(5))
        .await
        .expect("timeout capture reaches gated source");
    let timed_out = client()
        .get(broker_url(&format!("/entities?type={timeout_type}")))
        .header("NGSILD-Snapshot", timeout_id)
        .send()
        .await
        .expect("timed scoped request");
    pretty_assertions::assert_eq!(timed_out.status(), StatusCode::SERVICE_UNAVAILABLE);
    pretty_assertions::assert_eq!(timed_out.headers()["retry-after"], "1");
    timeout_gate.release();
    wait_for_snapshot_terminal(&client(), timeout_id, Duration::from_secs(8)).await;
    timeout_callback.shutdown().await.expect("timeout context source shuts down");
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_orphaned_preparation_claim_is_reclaimed_after_lease() {
    let _serialized_environment = clean_database().await;
    let callback = CallbackServer::start(MockServerConfig::from_environment().expect("orphan source configuration"))
        .await
        .expect("orphan context source starts");
    let path = CallbackPath::parse("/ngsi-ld/v1/entities").expect("static query path");
    let gate = ResponseGate::closed();
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, "application/ld+json".parse().expect("static content type"));
    callback
        .enqueue(
            RequestMatcher::new(Method::GET, path),
            MockResponse::builder()
                .status(AxumStatusCode::OK)
                .headers(headers)
                .body(Bytes::from_static(b"[]"))
                .gate(Some(gate.clone()))
                .build(),
        )
        .await;
    let endpoint = callback
        .endpoint(&CallbackPath::parse("/").expect("root path"))
        .expect("orphan source endpoint");
    let entity_type = "https://example.org/OrphanedCapture";
    pretty_assertions::assert_eq!(
        client()
            .post(broker_url("/csourceRegistrations"))
            .header("Content-Type", "application/ld+json")
            .json(&json!({
                "id": "urn:ngsi-ld:ContextSourceRegistration:orphaned-capture",
                "type": "ContextSourceRegistration",
                "endpoint": endpoint,
                "information": [{"entities": [{"type": entity_type}]}],
                "operations": ["queryEntity"],
                "@context": CORE_CONTEXT
            }))
            .send()
            .await
            .expect("orphan source registration")
            .status(),
        StatusCode::CREATED
    );
    let id = "urn:ngsi-ld:Snapshot:orphaned-claim";
    pretty_assertions::assert_eq!(
        create_snapshot_raw(
            &client(),
            &json!({"id": id, "type": "Snapshot", "snapshotQueries": [{"type": "Query", "entities": [{"type": entity_type}]}]})
        )
        .await
        .0,
        StatusCode::CREATED
    );
    callback.next_request(Duration::from_secs(5)).await.expect("original worker reaches source");
    let pool = PgPool::connect(&env::var("ANDROMEDA_TEST_DATABASE_URL").expect("Snapshot database URL"))
        .await
        .expect("Snapshot database connection");
    sqlx::query("UPDATE snapshots SET claimed_by = $2, claimed_at = NOW() - INTERVAL '1 minute', lease_until = NOW() - INTERVAL '1 second' WHERE id = $1")
        .bind(id)
        .bind(uuid::Uuid::new_v4())
        .execute(&pool)
        .await
        .expect("simulate abandoned expired lease");
    callback
        .next_request(Duration::from_secs(8))
        .await
        .expect("another replica reclaims expired preparation");
    gate.release();
    let terminal = wait_for_snapshot_terminal(&client(), id, Duration::from_secs(8)).await;
    pretty_assertions::assert_eq!(terminal["snapshotStatus"], "empty");
    pretty_assertions::assert_eq!(terminal["snapshotQueriesDetails"].as_array().expect("one detail").len(), 1);
    let published: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM snapshots WHERE id = $1 AND claimed_by IS NULL AND lease_until IS NULL")
        .bind(id)
        .fetch_one(&pool)
        .await
        .expect("one fenced promotion");
    pretty_assertions::assert_eq!(published, 1);
    callback.shutdown().await.expect("orphan context source shuts down");
}
