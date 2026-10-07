//! What a user `@context` may declare.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.7 states that the user `@context` shall
//! not "contain JSON-LD Scoped Contexts (see [2], section 4.1.8)", and that
//! parts of a user `@context` breaking that "should result in an error of type
//! `BadRequestData`, because JSON-LD Scoped Contexts and nested embedded
//! @context could be used to modify terms defined in the Core @context or to
//! reshape NGSI-LD Elements during the expansion of terms". Clauses 4.4 and
//! 4.5.1 restate it.
//!
//! The rule addresses the *user* `@context` alone. The Core `@context` is
//! protected and carries a scoped context of its own -- the `ngsildproof` term
//! of clause 4.5.2.2 -- and clause 5.13 makes hosting a context a resource
//! operation rather than a use of one, so neither is refused here.
//!
//! Each test serves its contexts from a mock origin under a path unique to the
//! run, and removes the `"Cached"` row the resolver leaves behind: those rows
//! are keyed by the origin URL rather than by the run's identifier prefix, so
//! the environment's own cleanup cannot reach them.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::hosted_context::{context_link, forget_cached, serve};
use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{jsonld_context_server::JsonLdContextServer, server_config::MockServerConfig};
use axum::http::HeaderMap;
use http::Method;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The problem type clause 5.5.7 gives a user `@context` declaring a scoped one.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// A term definition carrying a scoped `@context` that redefines a Core term,
/// which is the reshaping clause 5.5.7 names as the reason for the rule.
fn scoped_definition() -> Value {
    json!({"@id": "https://example.org/gear", "@context": {"observedAt": "https://example.org/rewritten"}})
}

/// Assert one refusal is the `BadRequestData` clause 5.5.7 asks for, and that
/// its detail names this rule rather than another admission.
fn assert_scoped_context_refused(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{scenario}: {problem}");
    let detail = problem["detail"].as_str().unwrap_or_default();
    assert!(detail.contains("scoped @context"), "{scenario}: {problem}");
}

/// One `Device` whose only attribute is the term under test.
fn device(id: &str, context: &Value) -> Value {
    json!({
        "id": id,
        "type": "Device",
        "gear": {"type": "Property", "value": 3},
        "@context": context
    })
}

/// Serve one context document under a path unique to this run.
async fn serve_context(server: &JsonLdContextServer, label: &str, document: &Value) -> String {
    let (_, url) = serve(server, &format!("scoped-user-context-{label}"), &document.to_string(), HeaderMap::new()).await;
    url
}

/// Send one `application/ld+json` body to a path the typed client does not
/// model.
async fn send_ld_json(client: &TestClient, method: Method, path: &str, body: &Value) -> TestResponse {
    client
        .raw(method, path)
        .header("content-type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("request")
}

/// Send one `application/json` body under the one `Link` context named.
async fn send_json_with_link_context(client: &TestClient, method: Method, path: &str, body: &Value, context: &str) -> TestResponse {
    client
        .raw(method, path)
        .header("content-type", "application/json")
        .header("link", context_link(context).as_str())
        .json(body)
        .send()
        .await
        .expect("request")
}

/// Clause 5.5.7: an inline user `@context` in the request body that defines a
/// term with a scoped `@context` is refused on the single-entity write path.
#[tokio::test]
async fn regression_scoped_user_context_an_inline_body_context_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:inline-scoped");

    let entity = device(&id, &json!([CORE_CONTEXT, {"gear": scoped_definition()}]));
    let response = client.post_entity(&entity).with_ld_json().send().await.expect("create entity request");
    assert_scoped_context_refused(&response, "clause 5.6.1 create entity");

    let missing = client.get_entity(&id, "", "application/ld+json", None).await.expect("retrieve entity request");
    assert_eq!(missing.status, StatusCode::NOT_FOUND, "a refused create stores nothing");

    scope.finish().await.expect("cleanup failed");
}

/// The same rule on a context the request names in a `Link` header, which has
/// to be fetched before anything can be said about it.
#[tokio::test]
async fn regression_scoped_user_context_a_link_header_context_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("callback host"))
        .await
        .expect("context server starts");

    let scoped = serve_context(&server, "link-scoped", &json!({"@context": {"gear": scoped_definition()}})).await;
    let plain = serve_context(&server, "link-plain", &json!({"@context": {"gear": "https://example.org/gear"}})).await;

    let refused_id = scope.id("Device:link-scoped");
    let refused = send_json_with_link_context(
        client,
        Method::POST,
        "/ngsi-ld/v1/entities",
        &json!({"id": refused_id, "type": "Device", "gear": {"type": "Property", "value": 3}}),
        &scoped,
    )
    .await;
    assert_scoped_context_refused(&refused, "a Link header context declaring a scoped @context");

    // The control: the same shape of context without the scoped definition is
    // fetched, used, and the write succeeds.
    let admitted_id = scope.id("Device:link-plain");
    let admitted = send_json_with_link_context(
        client,
        Method::POST,
        "/ngsi-ld/v1/entities",
        &json!({"id": admitted_id, "type": "Device", "gear": {"type": "Property", "value": 3}}),
        &plain,
    )
    .await;
    assert_eq!(admitted.status, StatusCode::CREATED, "a plain Link header context is used: {:?}", admitted.body);

    forget_cached(scope.database_pool(), &[scoped, plain]).await;
    server.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// The refusal is a property of the whole set of contexts a request selected,
/// so it cannot depend on the order they happen to be inspected in.
#[tokio::test]
async fn regression_scoped_user_context_a_scoped_context_in_the_second_of_two_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("callback host"))
        .await
        .expect("context server starts");

    let plain = serve_context(&server, "order-plain", &json!({"@context": {"axle": "https://example.org/axle"}})).await;
    let scoped = serve_context(&server, "order-scoped", &json!({"@context": {"gear": scoped_definition()}})).await;
    // A request names at most one context Link (JSON-LD 1.1 section 6.1), so a
    // set of two reaches the broker through one wrapper naming both (Annex C.8).
    let plain_first = serve_context(&server, "order-plain-first", &json!({"@context": [plain, scoped]})).await;
    let scoped_first = serve_context(&server, "order-scoped-first", &json!({"@context": [scoped, plain]})).await;

    let id = scope.id("Device:second-of-two");
    let response = send_json_with_link_context(
        client,
        Method::POST,
        "/ngsi-ld/v1/entities",
        &json!({"id": id, "type": "Device", "gear": {"type": "Property", "value": 3}}),
        &plain_first,
    )
    .await;
    assert_scoped_context_refused(&response, "the scoped @context is the second of two linked contexts");

    // And the same set in the other order, so the assertion is about the set
    // rather than about one arrangement of it.
    let reversed_id = scope.id("Device:second-of-two-reversed");
    let reversed = send_json_with_link_context(
        client,
        Method::POST,
        "/ngsi-ld/v1/entities",
        &json!({"id": reversed_id, "type": "Device", "gear": {"type": "Property", "value": 3}}),
        &scoped_first,
    )
    .await;
    assert_scoped_context_refused(&reversed, "the scoped @context is the first of two linked contexts");

    // The body form of the same set: an inline object beside a remote URL.
    let inline_id = scope.id("Device:second-of-two-inline");
    let inline = send_ld_json(
        client,
        Method::POST,
        "/ngsi-ld/v1/entities",
        &json!({
            "id": inline_id,
            "type": "Device",
            "gear": {"type": "Property", "value": 3},
            "@context": [CORE_CONTEXT, {"axle": "https://example.org/axle"}, scoped.clone()]
        }),
    )
    .await;
    assert_scoped_context_refused(&inline, "the scoped @context is the last element of a body @context array");

    forget_cached(scope.database_pool(), &[plain, scoped, plain_first, scoped_first]).await;
    server.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// A context document may name another remote context rather than declaring
/// terms itself. A scoped context one hop away modifies terms exactly as an
/// inline one does, so the walk has to follow the reference.
#[tokio::test]
async fn regression_scoped_user_context_a_nested_remote_reference_is_followed() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("callback host"))
        .await
        .expect("context server starts");

    let scoped = serve_context(&server, "nested-scoped", &json!({"@context": {"gear": scoped_definition()}})).await;
    let suite = serve_context(
        &server,
        "nested-suite",
        &json!({"@context": [scoped.clone(), {"axle": "https://example.org/axle"}]}),
    )
    .await;

    let id = scope.id("Device:nested-reference");
    let response = send_json_with_link_context(
        client,
        Method::POST,
        "/ngsi-ld/v1/entities",
        &json!({"id": id, "type": "Device", "axle": {"type": "Property", "value": 2}}),
        &suite,
    )
    .await;
    assert_scoped_context_refused(&response, "a scoped @context reached through a nested remote reference");

    forget_cached(scope.database_pool(), &[scoped, suite]).await;
    server.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// JSON-LD 1.1 section 4.1.10 merges an `@import`ed context into the
/// definition that imports it, scoped term definitions included, so a scoped
/// context reached through `@import` reshapes terms exactly as an inline one
/// does, so the scan has to follow the import.
#[tokio::test]
async fn regression_scoped_user_context_an_imported_scoped_context_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("callback host"))
        .await
        .expect("context server starts");

    let scoped = serve_context(&server, "import-scoped", &json!({"@context": {"gear": scoped_definition()}})).await;
    let plain = serve_context(&server, "import-plain", &json!({"@context": {"gear": "https://example.org/gear"}})).await;
    let importing = serve_context(
        &server,
        "import-linked",
        &json!({"@context": {"@import": scoped.clone(), "axle": "https://example.org/axle"}}),
    )
    .await;

    let inline_id = scope.id("Device:import-inline");
    let inline = send_ld_json(
        client,
        Method::POST,
        "/ngsi-ld/v1/entities",
        &device(
            &inline_id,
            &json!([CORE_CONTEXT, {"@import": scoped.clone(), "axle": "https://example.org/axle"}]),
        ),
    )
    .await;
    assert_scoped_context_refused(&inline, "an inline body @context importing a scoped @context");

    let linked_id = scope.id("Device:import-linked");
    let linked = send_json_with_link_context(
        client,
        Method::POST,
        "/ngsi-ld/v1/entities",
        &json!({"id": linked_id, "type": "Device", "axle": {"type": "Property", "value": 2}}),
        &importing,
    )
    .await;
    assert_scoped_context_refused(&linked, "a Link header @context importing a scoped @context");

    // Control: importing a context that declares only plain terms is used, so
    // the refusals above are about what the import brings in.
    let admitted_id = scope.id("Device:import-plain");
    let admitted = send_ld_json(
        client,
        Method::POST,
        "/ngsi-ld/v1/entities",
        &device(
            &admitted_id,
            &json!([CORE_CONTEXT, {"@import": plain.clone(), "axle": "https://example.org/axle"}]),
        ),
    )
    .await;
    assert_eq!(admitted.status, StatusCode::CREATED, "importing a plain context is used: {:?}", admitted.body);
    let stored = client
        .get_entity(&admitted_id, "", "application/ld+json", None)
        .await
        .expect("retrieve entity request");
    let entity = stored.json_value().expect("an entity is JSON");
    assert_eq!(
        entity["https://example.org/gear"]["value"],
        json!(3),
        "the imported term expanded to its IRI: {entity}"
    );

    forget_cached(scope.database_pool(), &[scoped, plain, importing]).await;
    server.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// Every entity write path runs the same admission before it forwards
/// anything, so none of them may take a scoped user `@context`.
#[tokio::test]
async fn regression_scoped_user_context_every_entity_write_path_refuses_it() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:write-paths");

    let seeded = json!({"id": id, "type": "Device", "gear": {"type": "Property", "value": 1}, "@context": [CORE_CONTEXT]});
    let created = client.post_entity(&seeded).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "seeding the target: {:?}", created.body);

    let context = json!([CORE_CONTEXT, {"gear": scoped_definition()}]);
    let fragment = json!({"gear": {"type": "Property", "value": 2}, "@context": context});
    let attribute_fragment = json!({"type": "Property", "value": 2, "@context": context});

    for (label, response) in [
        (
            "clause 5.6.3 append attributes",
            send_ld_json(client, Method::POST, &format!("/ngsi-ld/v1/entities/{id}/attrs"), &fragment).await,
        ),
        (
            "clause 5.6.2 update attributes",
            send_ld_json(client, Method::PATCH, &format!("/ngsi-ld/v1/entities/{id}/attrs"), &fragment).await,
        ),
        (
            "clause 5.6.4 partial update attribute",
            send_ld_json(client, Method::PATCH, &format!("/ngsi-ld/v1/entities/{id}/attrs/gear"), &attribute_fragment).await,
        ),
        (
            "clause 5.6.19 replace attribute",
            send_ld_json(client, Method::PUT, &format!("/ngsi-ld/v1/entities/{id}/attrs/gear"), &attribute_fragment).await,
        ),
        (
            "clause 5.6.18 replace entity",
            send_ld_json(
                client,
                Method::PUT,
                &format!("/ngsi-ld/v1/entities/{id}"),
                &json!({"id": id, "type": "Device", "gear": {"type": "Property", "value": 2}, "@context": context}),
            )
            .await,
        ),
        (
            "clause 5.6.17 merge entity",
            send_ld_json(client, Method::PATCH, &format!("/ngsi-ld/v1/entities/{id}"), &fragment).await,
        ),
    ] {
        assert_scoped_context_refused(&response, label);
    }

    let stored = client.get_entity(&id, "", "application/ld+json", None).await.expect("retrieve entity request");
    let entity = stored.json_value().expect("an entity is JSON");
    assert_eq!(entity["gear"]["value"], json!(1), "no refused write reached the store: {entity}");

    scope.finish().await.expect("cleanup failed");
}

/// A scoped user `@context` belongs to the request rather than to any one
/// entity in a batch array, so clauses 5.6.7.4 to 5.6.9.4 and 5.6.20.4 refuse
/// the operation instead of projecting the failure onto each entity.
#[tokio::test]
async fn regression_scoped_user_context_every_batch_operation_refuses_the_request() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (label, path) in [
        ("clause 5.6.7 batch create", "/ngsi-ld/v1/entityOperations/create"),
        ("clause 5.6.8 batch upsert", "/ngsi-ld/v1/entityOperations/upsert"),
        ("clause 5.6.9 batch update", "/ngsi-ld/v1/entityOperations/update"),
        ("clause 5.6.20 batch merge", "/ngsi-ld/v1/entityOperations/merge"),
    ] {
        let id = scope.id(&format!("Device:batch-{}", label.replace(' ', "-")));
        let body = json!([device(&id, &json!([CORE_CONTEXT, {"gear": scoped_definition()}]))]);
        let response = send_ld_json(client, Method::POST, path, &body).await;
        assert_scoped_context_refused(&response, label);
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 5.6.11.4 and 5.6.12.4 open with the same clause 5.5.4 validation,
/// and clause 5.6.14 modifies one instance: none of the three may take a scoped
/// user `@context` either.
#[tokio::test]
async fn regression_scoped_user_context_temporal_writes_are_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:temporal-scoped");
    let context = json!([CORE_CONTEXT, {"gear": scoped_definition()}]);

    let created = client
        .create_temporal_entity(&json!({
            "id": id,
            "type": "Device",
            "gear": [{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00Z"}],
            "@context": context
        }))
        .await
        .expect("create temporal entity request");
    assert_scoped_context_refused(&created, "clause 5.6.11 create or update temporal entity");

    let appended = send_ld_json(
        client,
        Method::POST,
        &format!("/ngsi-ld/v1/temporal/entities/{id}/attrs"),
        &json!({"gear": [{"type": "Property", "value": 2, "observedAt": "2024-01-02T00:00:00Z"}], "@context": context}),
    )
    .await;
    assert_scoped_context_refused(&appended, "clause 5.6.12 add temporal attributes");

    let instance = send_ld_json(
        client,
        Method::PATCH,
        &format!("/ngsi-ld/v1/temporal/entities/{id}/attrs/gear/urn:ngsi-ld:AttributeInstance:probe"),
        &json!({"gear": [{"type": "Property", "value": 5}], "@context": context}),
    )
    .await;
    assert_scoped_context_refused(&instance, "clause 5.6.14 modify attribute instance");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.13.4 stores a client-supplied `@context` as a resource of its own.
/// Hosting a context is not using it as a request `@context`, so a document
/// that happens to declare a scoped context is stored and served back
/// unchanged -- what clause 5.5.7 refuses is a request that *expands against*
/// one.
#[tokio::test]
async fn regression_scoped_user_context_hosting_a_context_that_declares_one_is_admitted() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let document = json!({"@context": {"gear": scoped_definition()}});
    let created = client.create_jsonld_context(&document).await.expect("create jsonldContext request");
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "clause 5.13.4 stores the document a client hands over: {:?}",
        created.body
    );

    let location = created
        .headers
        .get("location")
        .and_then(|value| value.to_str().ok())
        .expect("clause 5.13.4 answers with the new Context's location");
    let identifier = location.rsplit('/').next().expect("the location names the Context").to_owned();

    let served = client.retrieve_jsonld_context(&identifier).await.expect("retrieve jsonldContext request");
    assert_eq!(served.status, StatusCode::OK, "the hosted context is served back: {:?}", served.body);
    let payload = served.json_value().expect("a hosted context is JSON");
    assert_eq!(
        payload["@context"]["gear"]["@context"],
        json!({"observedAt": "https://example.org/rewritten"}),
        "clause 5.13.2.3 keeps the @context subtree verbatim: {payload}"
    );

    let removed = client.delete_jsonld_context(&identifier).await.expect("delete jsonldContext request");
    assert_eq!(removed.status, StatusCode::NO_CONTENT, "the hosted context is removable: {:?}", removed.body);

    scope.finish().await.expect("cleanup failed");
}

/// A remote scoped context, and a plain one of the same shape, both served for
/// one run's read-path tests.
async fn scoped_and_plain(server: &JsonLdContextServer, label: &str) -> (String, String) {
    let scoped = serve_context(server, &format!("{label}-scoped"), &json!({"@context": {"gear": scoped_definition()}})).await;
    let plain = serve_context(server, &format!("{label}-plain"), &json!({"@context": {"gear": "https://example.org/gear"}})).await;
    (scoped, plain)
}

/// Clause 5.5.7 addresses the request's `@context` on every operation that
/// expands one, so the retrieval of clause 5.7.1 refuses a scoped user
/// `@context` named in a `Link` header before it looks the entity up.
#[tokio::test]
async fn regression_scoped_user_context_a_get_retrieve_refuses_a_scoped_context() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("callback host"))
        .await
        .expect("context server starts");
    let (scoped, plain) = scoped_and_plain(&server, "get-retrieve").await;

    let id = scope.id("Device:get-retrieve");
    let seeded = json!({"id": id, "type": "Device", "gear": {"type": "Property", "value": 1}, "@context": [CORE_CONTEXT]});
    let created = client.post_entity(&seeded).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "seeding the target: {:?}", created.body);

    let refused = send_json_with_link_context(client, Method::GET, &format!("/ngsi-ld/v1/entities/{id}"), &Value::Null, &scoped).await;
    assert_scoped_context_refused(&refused, "retrieve entity under a scoped @context");

    // Control: the same read under a plain user context is answered.
    let admitted = send_json_with_link_context(client, Method::GET, &format!("/ngsi-ld/v1/entities/{id}"), &Value::Null, &plain).await;
    assert_eq!(admitted.status, StatusCode::OK, "retrieve under a plain context: {:?}", admitted.body);

    forget_cached(scope.database_pool(), &[scoped, plain]).await;
    server.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// The GET binding of Query Entities (clause 5.7.2) refuses a scoped user
/// `@context` named in a `Link` header before it runs.
#[tokio::test]
async fn regression_scoped_user_context_a_get_query_refuses_a_scoped_context() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("callback host"))
        .await
        .expect("context server starts");
    let (scoped, plain) = scoped_and_plain(&server, "get-query").await;

    let refused = send_json_with_link_context(client, Method::GET, "/ngsi-ld/v1/entities?type=Device", &Value::Null, &scoped).await;
    assert_scoped_context_refused(&refused, "query entities under a scoped @context");

    let admitted = send_json_with_link_context(client, Method::GET, "/ngsi-ld/v1/entities?type=Device", &Value::Null, &plain).await;
    assert_eq!(admitted.status, StatusCode::OK, "query under a plain context: {:?}", admitted.body);

    forget_cached(scope.database_pool(), &[scoped, plain]).await;
    server.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// The POST binding of Query Entities (clause 5.7.2) carries its `@context` in
/// the body, and it is refused there once merged, exactly as the GET binding's
/// `Link` context is.
#[tokio::test]
async fn regression_scoped_user_context_a_post_query_refuses_a_scoped_context() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let refused = send_ld_json(
        client,
        Method::POST,
        "/ngsi-ld/v1/entityOperations/query",
        &json!({"type": "Query", "entities": [{"type": "Device"}], "@context": [CORE_CONTEXT, {"gear": scoped_definition()}]}),
    )
    .await;
    assert_scoped_context_refused(&refused, "POST query under a scoped body @context");

    let admitted = send_ld_json(
        client,
        Method::POST,
        "/ngsi-ld/v1/entityOperations/query",
        &json!({"type": "Query", "entities": [{"type": "Device"}], "@context": [CORE_CONTEXT, {"gear": "https://example.org/gear"}]}),
    )
    .await;
    assert_eq!(admitted.status, StatusCode::OK, "POST query under a plain body @context: {:?}", admitted.body);

    scope.finish().await.expect("cleanup failed");
}

/// Query Temporal Evolution of Entities (clause 5.7.4) refuses a scoped user
/// `@context` on its read path too.
#[tokio::test]
async fn regression_scoped_user_context_a_temporal_query_refuses_a_scoped_context() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("callback host"))
        .await
        .expect("context server starts");
    let (scoped, plain) = scoped_and_plain(&server, "temporal-query").await;

    let window = "type=Device&timerel=after&timeAt=2000-01-01T00:00:00Z";
    let refused = send_json_with_link_context(client, Method::GET, &format!("/ngsi-ld/v1/temporal/entities?{window}"), &Value::Null, &scoped).await;
    assert_scoped_context_refused(&refused, "temporal query under a scoped @context");

    let admitted = send_json_with_link_context(client, Method::GET, &format!("/ngsi-ld/v1/temporal/entities?{window}"), &Value::Null, &plain).await;
    assert_eq!(admitted.status, StatusCode::OK, "temporal query under a plain context: {:?}", admitted.body);

    forget_cached(scope.database_pool(), &[scoped, plain]).await;
    server.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// Create Subscription (clause 5.8.1) validates the request `@context` it will
/// render notifications against, and refuses a scoped one (clause 5.5.7).
#[tokio::test]
async fn regression_scoped_user_context_a_subscription_create_refuses_a_scoped_context() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let scoped_id = scope.id("Subscription:create-scoped");

    let refused = send_ld_json(
        client,
        Method::POST,
        "/ngsi-ld/v1/subscriptions",
        &json!({
            "id": scoped_id,
            "type": "Subscription",
            "entities": [{"type": "Device"}],
            "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}},
            "@context": [CORE_CONTEXT, {"gear": scoped_definition()}]
        }),
    )
    .await;
    assert_scoped_context_refused(&refused, "create subscription under a scoped @context");

    let plain_id = scope.id("Subscription:create-plain");
    let admitted = send_ld_json(
        client,
        Method::POST,
        "/ngsi-ld/v1/subscriptions",
        &json!({
            "id": plain_id,
            "type": "Subscription",
            "entities": [{"type": "Device"}],
            "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}},
            "@context": [CORE_CONTEXT, {"gear": "https://example.org/gear"}]
        }),
    )
    .await;
    assert_eq!(
        admitted.status,
        StatusCode::CREATED,
        "create subscription under a plain @context: {:?}",
        admitted.body
    );

    scope.finish().await.expect("cleanup failed");
}

/// Update Subscription (clause 5.8.3) runs the same admission, so a scoped user
/// `@context` on the update is refused and leaves the subscription unchanged.
#[tokio::test]
async fn regression_scoped_user_context_a_subscription_update_refuses_a_scoped_context() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Subscription:update-scoped");

    let created = send_ld_json(
        client,
        Method::POST,
        "/ngsi-ld/v1/subscriptions",
        &json!({
            "id": id,
            "type": "Subscription",
            "entities": [{"type": "Device"}],
            "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "seeding the subscription: {:?}", created.body);

    let refused = send_ld_json(
        client,
        Method::PATCH,
        &format!("/ngsi-ld/v1/subscriptions/{id}"),
        &json!({"description": "rescoped", "@context": [CORE_CONTEXT, {"gear": scoped_definition()}]}),
    )
    .await;
    assert_scoped_context_refused(&refused, "update subscription under a scoped @context");

    // Control: a plain-context update is applied.
    let admitted = send_ld_json(
        client,
        Method::PATCH,
        &format!("/ngsi-ld/v1/subscriptions/{id}"),
        &json!({"description": "still fine", "@context": [CORE_CONTEXT]}),
    )
    .await;
    assert_eq!(
        admitted.status,
        StatusCode::NO_CONTENT,
        "update subscription under a plain @context: {:?}",
        admitted.body
    );

    scope.finish().await.expect("cleanup failed");
}

/// Create Context Source Registration (clause 5.9.2) refuses a scoped user
/// `@context` before it expands the registration's footprint.
#[tokio::test]
async fn regression_scoped_user_context_a_registration_create_refuses_a_scoped_context() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let scoped_id = scope.id("ContextSourceRegistration:create-scoped");

    let refused = send_ld_json(
        client,
        Method::POST,
        "/ngsi-ld/v1/csourceRegistrations",
        &json!({
            "id": scoped_id,
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Device"}]}],
            "endpoint": "http://127.0.0.1:9",
            "@context": [CORE_CONTEXT, {"gear": scoped_definition()}]
        }),
    )
    .await;
    assert_scoped_context_refused(&refused, "create registration under a scoped @context");

    let plain_id = scope.id("ContextSourceRegistration:create-plain");
    let admitted = send_ld_json(
        client,
        Method::POST,
        "/ngsi-ld/v1/csourceRegistrations",
        &json!({
            "id": plain_id,
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Device"}]}],
            "endpoint": "http://127.0.0.1:9",
            "@context": [CORE_CONTEXT, {"gear": "https://example.org/gear"}]
        }),
    )
    .await;
    assert_eq!(
        admitted.status,
        StatusCode::CREATED,
        "create registration under a plain @context: {:?}",
        admitted.body
    );

    scope.finish().await.expect("cleanup failed");
}

/// The control for the whole rule: a user `@context` that declares ordinary
/// terms, inline and remote, is used exactly as before.
#[tokio::test]
async fn regression_scoped_user_context_an_ordinary_user_context_is_still_used() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("callback host"))
        .await
        .expect("context server starts");

    let remote = serve_context(&server, "control-plain", &json!({"@context": {"axle": "https://example.org/axle"}})).await;
    let id = scope.id("Device:ordinary-context");

    let created = send_ld_json(
        client,
        Method::POST,
        "/ngsi-ld/v1/entities",
        &json!({
            "id": id,
            "type": "Device",
            "gear": {"type": "Property", "value": 3},
            "axle": {"type": "Property", "value": 2},
            "@context": [CORE_CONTEXT, remote.clone(), {"gear": "https://example.org/gear"}]
        }),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "an ordinary user @context is used: {:?}", created.body);

    let stored = send_json_with_link_context(client, Method::GET, &format!("/ngsi-ld/v1/entities/{id}"), &Value::Null, &remote).await;
    assert_eq!(stored.status, StatusCode::OK, "retrieving under the same context: {:?}", stored.body);
    let entity = stored.json_value().expect("an entity is JSON");
    assert_eq!(entity["axle"]["value"], json!(2), "the remote context's term compacts back: {entity}");

    forget_cached(scope.database_pool(), &[remote]).await;
    server.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}
