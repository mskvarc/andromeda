//! List @contexts (clause 5.13.3, binding 6.29.3.2).

use crate::{
    broker_endpoints::broker_url,
    context_api::{create_context, extract_id, list_contexts, setup_cached_context},
    ngsi_requests::{create_subscription, delete_subscription},
    private_tenant::{provision_tenant, tenant_client},
    problem_types::BAD_REQUEST_DATA_TYPE,
};
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// Assert listing contains no non-core context URIs.
fn assert_no_non_core(contexts: &[Value]) {
    let non_core: Vec<_> = contexts.iter().filter(|v| v.as_str().is_none_or(|s| !s.contains("core-context"))).collect();
    assert!(non_core.is_empty(), "expected no non-core contexts, got {non_core:?}");
}

/// Assert listing names the stored @context `uri` (a `Location` or serving
/// URL) addresses. A non-default tenant's serving URL carries its tenant as a
/// query parameter, so entries are compared by identifier.
fn assert_contains(contexts: &[Value], uri: &str) {
    let identifier = extract_id(uri);
    assert!(
        contexts.iter().any(|v| v.as_str().is_some_and(|s| extract_id(s) == identifier)),
        "should contain context {identifier}: {contexts:?}"
    );
}

/// Assert listing contains all given URIs.
fn assert_contains_all(contexts: &[Value], uris: &[String]) {
    for uri in uris {
        assert_contains(contexts, uri);
    }
}

/// Create one hosted context and return its URI.
async fn create_one_context(c: &reqwest::Client) -> String {
    let body = json!({
        "@context": {
            "A1": "urn:ngsi-ld:attributes:A1",
            "A2": "urn:ngsi-ld:attributes:A2",
            "A3": "urn:ngsi-ld:attributes:A3"
        }
    });
    let (status, location, _) = create_context(c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    location.unwrap()
}

/// Create 3 hosted contexts and return their URIs.
async fn create_three_contexts(c: &reqwest::Client) -> Vec<String> {
    let payloads = vec![
        json!({"@context": {"A1": "urn:ngsi-ld:attributes:A1", "A2": "urn:ngsi-ld:attributes:A2", "A3": "urn:ngsi-ld:attributes:A3"}}),
        json!({"@context": {"B1": "urn:ngsi-ld:attributes:B1", "B2": "urn:ngsi-ld:attributes:B2", "B3": "urn:ngsi-ld:attributes:B3"}}),
        json!({"@context": {"C1": "urn:ngsi-ld:attributes:C1", "C2": "urn:ngsi-ld:attributes:C2", "C3": "urn:ngsi-ld:attributes:C3"}}),
    ];
    let mut uris = Vec::new();
    for p in &payloads {
        let (status, location, _) = create_context(c, p).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
        uris.push(location.unwrap());
    }
    uris
}

/// Create a subscription that generates an `ImplicitlyCreated` context.
async fn create_implicit_context_for_list(c: &reqwest::Client) -> String {
    let sub = json!({
        "type": "Subscription",
        "entities": [{"type": "T1"}],
        "notification": {"endpoint": {"uri": "http://127.0.0.1:1/notify"}},
        "@context": [{ "T1": "https://example.org/list/T1" }, { "speed": "https://example.org/list/speed" }]
    });
    let (status, location) = create_subscription(c, &sub).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    extract_id(location.as_ref().unwrap())
}

#[tokio::test]
async fn list_no_contexts_no_params() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let (status, contexts) = list_contexts(&c, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_no_non_core(&contexts);
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_no_contexts_kind_hosted() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let (status, contexts) = list_contexts(&c, "kind=Hosted").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_no_non_core(&contexts);
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_no_contexts_kind_cached() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let (status, contexts) = list_contexts(&c, "kind=Cached").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_no_non_core(&contexts);
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_no_contexts_kind_implicitly_created() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let (status, contexts) = list_contexts(&c, "kind=ImplicitlyCreated").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_no_non_core(&contexts);
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_no_contexts_details_false() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let (status, contexts) = list_contexts(&c, "details=false").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_no_non_core(&contexts);
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_no_contexts_details_false_kind_hosted() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let (status, contexts) = list_contexts(&c, "details=false&kind=Hosted").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_no_non_core(&contexts);
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_no_contexts_details_false_kind_cached() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let (status, contexts) = list_contexts(&c, "details=false&kind=Cached").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_no_non_core(&contexts);
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_no_contexts_details_false_kind_implicitly_created() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let (status, contexts) = list_contexts(&c, "details=false&kind=ImplicitlyCreated").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_no_non_core(&contexts);
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_one_context_no_params() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let uri = create_one_context(&c).await;
    let (status, contexts) = list_contexts(&c, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_contains(&contexts, &uri);
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_one_context_kind_hosted() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let uri = create_one_context(&c).await;
    let (status, contexts) = list_contexts(&c, "kind=Hosted").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_contains(&contexts, &uri);
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_one_context_details_false() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let uri = create_one_context(&c).await;
    let (status, contexts) = list_contexts(&c, "details=false").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_contains(&contexts, &uri);
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_one_context_details_false_kind_hosted() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let uri = create_one_context(&c).await;
    let (status, contexts) = list_contexts(&c, "details=false&kind=Hosted").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_contains(&contexts, &uri);
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_multiple_contexts_no_params() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let uris = create_three_contexts(&c).await;
    let (status, contexts) = list_contexts(&c, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_contains_all(&contexts, &uris);
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_multiple_contexts_kind_hosted() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let uris = create_three_contexts(&c).await;
    let (status, contexts) = list_contexts(&c, "kind=Hosted").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_contains_all(&contexts, &uris);
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_multiple_contexts_details_false() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let uris = create_three_contexts(&c).await;
    let (status, contexts) = list_contexts(&c, "details=false").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_contains_all(&contexts, &uris);
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_multiple_contexts_details_false_kind_hosted() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let uris = create_three_contexts(&c).await;
    let (status, contexts) = list_contexts(&c, "details=false&kind=Hosted").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_contains_all(&contexts, &uris);
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_invalid_kind_returns_400() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let resp = c.get(broker_url("jsonldContexts?kind=other")).send().await.expect("request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_invalid_details_returns_400() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let resp = c.get(broker_url("jsonldContexts?details=other")).send().await.expect("request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_invalid_kind_with_details_true_returns_400() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let resp = c
        .get(broker_url("jsonldContexts?details=true&kind=other"))
        .send()
        .await
        .expect("request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_invalid_details_with_kind_hosted_returns_400() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let resp = c
        .get(broker_url("jsonldContexts?details=other&kind=Hosted"))
        .send()
        .await
        .expect("request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_invalid_details_and_kind_returns_400() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let resp = c
        .get(broker_url("jsonldContexts?details=other&kind=other"))
        .send()
        .await
        .expect("request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_details_true_no_kind() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;

    let body = json!({
        "@context": {
            "A1": "urn:ngsi-ld:attributes:A1",
            "A2": "urn:ngsi-ld:attributes:A2",
            "A3": "urn:ngsi-ld:attributes:A3"
        }
    });
    let (status, _, _) = create_context(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, contexts) = list_contexts(&c, "details=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let hosted = contexts.iter().find(|c| c.get("kind").and_then(|k| k.as_str()) == Some("Hosted"));
    assert!(hosted.is_some(), "should have a Hosted context in detailed list");

    let entry = hosted.unwrap();
    assert!(entry.get("URL").is_some(), "should have URL");
    assert!(entry.get("kind").is_some(), "should have kind");
    assert!(entry.get("createdAt").is_some(), "should have createdAt");
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_details_true_kind_hosted() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;

    let body = json!({
        "@context": {
            "A1": "urn:ngsi-ld:attributes:A1",
            "A2": "urn:ngsi-ld:attributes:A2",
            "A3": "urn:ngsi-ld:attributes:A3"
        }
    });
    let (status, _, _) = create_context(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, contexts) = list_contexts(&c, "details=true&kind=Hosted").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let hosted = contexts.iter().find(|c| c.get("kind").and_then(|k| k.as_str()) == Some("Hosted"));
    assert!(hosted.is_some(), "should have a Hosted context");

    let entry = hosted.unwrap();
    assert!(entry.get("URL").is_some());
    assert!(entry.get("kind").is_some());
    assert!(entry.get("createdAt").is_some());
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_implicit_no_params() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let sub_id = create_implicit_context_for_list(&c).await;

    let (status, contexts) = list_contexts(&c, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(
        contexts.iter().any(|v| v.as_str().is_none_or(|s| !s.contains("core-context"))),
        "should have non-core contexts (implicit)"
    );

    delete_subscription(&c, &sub_id).await;
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_implicit_kind_hosted_is_empty() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let sub_id = create_implicit_context_for_list(&c).await;

    let (status, contexts) = list_contexts(&c, "kind=Hosted").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(contexts.is_empty(), "Hosted filter should not include implicit contexts");

    delete_subscription(&c, &sub_id).await;
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_implicit_kind_cached() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let sub_id = create_implicit_context_for_list(&c).await;

    let (status, _contexts) = list_contexts(&c, "kind=Cached").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    delete_subscription(&c, &sub_id).await;
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_implicit_kind_implicitly_created() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let sub_id = create_implicit_context_for_list(&c).await;

    let (status, contexts) = list_contexts(&c, "kind=ImplicitlyCreated").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(!contexts.is_empty(), "should have ImplicitlyCreated contexts");

    delete_subscription(&c, &sub_id).await;
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_implicit_details_false() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let sub_id = create_implicit_context_for_list(&c).await;

    let (status, contexts) = list_contexts(&c, "details=false").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(
        contexts.iter().any(|v| v.as_str().is_none_or(|s| !s.contains("core-context"))),
        "should have non-core contexts (implicit)"
    );

    delete_subscription(&c, &sub_id).await;
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_implicit_details_false_kind_hosted_is_empty() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let sub_id = create_implicit_context_for_list(&c).await;

    let (status, contexts) = list_contexts(&c, "details=false&kind=Hosted").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(contexts.is_empty(), "Hosted filter should not include implicit contexts");

    delete_subscription(&c, &sub_id).await;
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_implicit_details_false_kind_cached() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let sub_id = create_implicit_context_for_list(&c).await;

    let (status, _contexts) = list_contexts(&c, "details=false&kind=Cached").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    delete_subscription(&c, &sub_id).await;
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_implicit_details_false_kind_implicitly_created() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let sub_id = create_implicit_context_for_list(&c).await;

    let (status, contexts) = list_contexts(&c, "details=false&kind=ImplicitlyCreated").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(!contexts.is_empty(), "should have ImplicitlyCreated contexts");

    delete_subscription(&c, &sub_id).await;
    env.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_cached_contexts_with_details() {
    let env = TenantScopedEnvironment::from_suite("jsonld_context").await.expect("private tenant environment");
    let c = tenant_client(env.tenant());
    provision_tenant(&c).await;
    let (server, ctx_url) = setup_cached_context(&c).await;

    let (status, contexts) = list_contexts(&c, "details=true&kind=Cached").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let cached = contexts.iter().find(|c| c.get("URL").and_then(|u| u.as_str()).is_some_and(|u| u == ctx_url));
    assert!(cached.is_some(), "should find cached context in detailed list");

    let entry = cached.unwrap();
    pretty_assertions::assert_eq!(entry["kind"], "Cached");
    assert!(entry.get("URL").is_some());
    assert!(entry.get("createdAt").is_some());

    server.shutdown().await;
    env.finish().await.expect("cleanup failed");
}
