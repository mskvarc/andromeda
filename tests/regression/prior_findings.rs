//! Regression tests for review findings predating the spec-conformance sweep.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::header::jsonld_link_header;
use andromeda_test_fixtures::context::{CORE_CONTEXT, TEST_SUITE_CONTEXT};
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{notification_server::NotificationServer, server_config::MockServerConfig};
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::json;
use std::time::Duration;

/// R2-50: a `receiverInfo` header that is a valid HTTP header must reach the
/// notification endpoint. The delivery path applies each `receiverInfo` entry;
/// the fix additionally surfaces (rather than silently drops) any entry that
/// cannot become a header, so an `Authorization` token is never quietly lost,
/// which would deliver the notification unauthenticated.
#[tokio::test]
async fn regression_r2_50_receiver_info_authorization_header_reaches_the_endpoint() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");

    let subscription_id = scope.id("Subscription:r2-50-receiverinfo");
    let subscription = json!({
        "id": subscription_id,
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "notification": {
            "endpoint": {
                "uri": receiver.endpoint().expect("notification endpoint must be valid"),
                "accept": "application/json",
                "receiverInfo": [{"key": "Authorization", "value": "Bearer secret-token-r2-50"}]
            }
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client.create_subscription(&subscription).await.expect("create subscription failed");
    assert_eq!(response.status, StatusCode::CREATED, "subscription create: {:?}", response.body);

    // Creating a matching entity triggers a notification.
    let entity = json!({
        "id": scope.id("Building:r2-50"),
        "type": "Building",
        "temperature": {"type": "Property", "value": 21.5},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED);

    let captured = receiver.next_request(Duration::from_secs(15)).await.expect("matching notification must arrive");
    let authorization = captured
        .headers
        .get("authorization")
        .expect("the receiverInfo Authorization header must reach the endpoint, not be silently dropped");
    assert_eq!(authorization.to_str().expect("ascii header"), "Bearer secret-token-r2-50");

    receiver.shutdown().await.expect("notification server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// R2-113: a JSON-LD context `Link` header whose parameters carry an escaped
/// quote (RFC 8288 quoted-pair) and a semicolon inside the quotes must still be
/// parsed correctly, so the context is applied and the entity's compact `type`
/// resolves. A parser that toggled quote state on the escaped quote severed the
/// value and could shadow the real `rel`, losing the context.
#[tokio::test]
async fn regression_r2_113_context_link_with_an_escaped_quote_still_resolves_the_context() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:r2-113");

    // A `title` parameter carrying an escaped quote and a semicolon inside its
    // quotes, placed before the real `rel`. A parser that ended the quoted value
    // at the escaped quote would split on the inner `;` and expose a spurious
    // `rel`, hiding the genuine context link.
    let link = format!("<{TEST_SUITE_CONTEXT}>; title=\"a\\\"b; rel=nonsense\"; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"");

    // Body carries no `@context`, so the type can only compact if the Link
    // context was parsed and applied.
    let body = json!({
        "id": id,
        "type": "Building",
        "temperature": {"type": "Property", "value": 19.0}
    });
    let created = client
        .raw(Method::POST, "/ngsi-ld/v1/entities")
        .header("Content-Type", "application/json")
        .header("Link", &link)
        .json(&body)
        .send()
        .await
        .expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create with tricky Link: {:?}", created.body);

    // Retrieving under the same context must compact the type back to `Building`
    // -- proof the Link context was the one applied on write.
    let retrieved = client
        .raw(Method::GET, &format!("/ngsi-ld/v1/entities/{}", urlencoding::encode(&id)))
        .header("Accept", "application/ld+json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test context Link must be valid"))
        .send()
        .await
        .expect("retrieve entity failed");
    assert_eq!(retrieved.status, StatusCode::OK);
    let retrieved_body = retrieved.json_value().expect("retrieved entity must be valid JSON");
    assert_eq!(
        retrieved_body["type"], "Building",
        "the Link context must have been applied on write: {:?}",
        retrieved_body
    );

    scope.finish().await.expect("cleanup failed");
}

/// R2-24: a POST query body's `entities[]` is a union of independent selectors
/// (clause 5.2.11), not a flattened cross product. `{id:A,type:T1}` OR
/// `{id:B,type:T2}` must not match an entity `(id:A, type:T2)` that pairs one
/// selector's id with another's type -- the pairing the old flattened
/// `id IN (A,B) AND type IN (T1,T2)` form wrongly admitted.
#[tokio::test]
async fn regression_r2_24_post_query_selectors_are_a_union_not_a_cross_product() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id_a = scope.id("Vehicle:r2-24-a");
    let id_b = scope.id("Building:r2-24-b");

    // The cross-product trap: selector 1's id (A) carried by an entity of
    // selector 2's type (Building). Neither `{id:A,type:Vehicle}` nor
    // `{id:B,type:Building}` names this pairing, so the union must exclude it.
    let cross = json!({"id": id_a, "type": "Building", "@context": [TEST_SUITE_CONTEXT]});
    // A clean match for selector 2.
    let matching = json!({"id": id_b, "type": "Building", "@context": [TEST_SUITE_CONTEXT]});
    assert_eq!(client.post_entity(&cross).send().await.expect("create cross").status, StatusCode::CREATED);
    assert_eq!(client.post_entity(&matching).send().await.expect("create matching").status, StatusCode::CREATED);

    let query_body = json!({
        "type": "Query",
        "entities": [
            {"id": id_a, "type": "Vehicle"},
            {"id": id_b, "type": "Building"}
        ]
    });
    let response = client
        .raw(Method::POST, "/ngsi-ld/v1/entityOperations/query")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test context Link must be valid"))
        .json(&query_body)
        .send()
        .await
        .expect("post query failed");
    assert_eq!(response.status, StatusCode::OK, "query: {:?}", response.body);

    let response_body = response.json_value().expect("query response must be valid JSON");
    let returned: Vec<String> = response_body
        .as_array()
        .expect("array body")
        .iter()
        .filter_map(|entity| entity["id"].as_str().map(str::to_string))
        .collect();
    assert!(returned.contains(&id_b), "selector {{id:B,type:Building}} must match entity B: {returned:?}");
    assert!(
        !returned.contains(&id_a),
        "the cross pairing (id:A, type:Building) matches neither selector and must be excluded: {returned:?}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// R2-55: clause 4.19 admits a conjunction only parenthesised
/// (`OrScopeQ = "(" ScopeQ *(andOp ScopeQ) ")" / ScopeQ`). A subscription that
/// stores a `scopeQ` conjunction `/A;/B` must render it back as `(/A;/B)` -- text
/// a conforming peer broker accepts -- not the bare `/A;/B` the renderer used to
/// emit.
#[tokio::test]
async fn regression_r2_55_stored_scope_query_conjunction_renders_parenthesised() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let subscription_id = scope.id("Subscription:r2-55-scope");
    let subscription = json!({
        "id": subscription_id,
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "scopeQ": "/Madrid;/Barcelona",
        "notification": {"endpoint": {"uri": "http://localhost:9/notify", "accept": "application/json"}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.create_subscription(&subscription).await.expect("create subscription failed");
    assert_eq!(created.status, StatusCode::CREATED, "subscription create: {:?}", created.body);

    let retrieved = client
        .raw(Method::GET, &format!("/ngsi-ld/v1/subscriptions/{}", urlencoding::encode(&subscription_id)))
        .header("Accept", "application/json")
        .send()
        .await
        .expect("get subscription failed");
    assert_eq!(retrieved.status, StatusCode::OK, "get subscription: {:?}", retrieved.body);
    let retrieved_body = retrieved.json_value().expect("retrieved subscription must be valid JSON");
    assert_eq!(
        retrieved_body["scopeQ"], "(/Madrid;/Barcelona)",
        "a stored scopeQ conjunction must render parenthesised (clause 4.19): {:?}",
        retrieved_body["scopeQ"]
    );

    scope.finish().await.expect("cleanup failed");
}

/// R2-73: the deployment core-context pin (clause 4.3.6.8) governs only
/// expansion; a response advertises the stable versionless core context, not
/// the version the deployment pinned, so a client is never tied to it.
#[tokio::test]
async fn regression_r2_73_response_advertises_the_versionless_core_context() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:r2-73");
    let entity = json!({"id": id, "type": "Device", "temp": {"type": "Property", "value": 21}, "@context": [CORE_CONTEXT]});
    assert_eq!(client.post_entity(&entity).send().await.expect("create").status, StatusCode::CREATED);

    // No explicit request context, so the response names the core context
    // (clause 5.5.7 AppendIfNoExplicit) rather than echoing one the client sent.
    let response = client.get_entity(&id, "", "application/ld+json", None).await.expect("get");
    assert_eq!(response.status, StatusCode::OK);
    let response_body = response.json_value().expect("retrieved entity must be valid JSON");
    let context = response_body["@context"].as_array().expect("an ld+json body carries an @context array");
    assert!(
        context
            .iter()
            .any(|c| c.as_str() == Some("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld")),
        "the response must advertise the versionless core context: {context:?}"
    );
    assert!(
        !context.iter().any(|c| c.as_str().is_some_and(|s| s.contains("core-context-v1.9"))),
        "the response must not leak the pinned v1.9 core context: {context:?}"
    );
    scope.finish().await.expect("cleanup failed");
}

/// R2-71: clause 5.2.23 fixes a POST query body's `type` as `Query`; a body that
/// names a different resource type is rejected, not accepted and ignored.
#[tokio::test]
async fn regression_r2_71_post_query_body_typed_other_than_query_is_rejected() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let body = json!({"type": "Subscription", "entities": [{"type": "Building"}]});
    let response = client
        .raw(Method::POST, "/ngsi-ld/v1/entityOperations/query")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test context Link must be valid"))
        .json(&body)
        .send()
        .await
        .expect("post query failed");
    assert_eq!(
        response.status,
        StatusCode::BAD_REQUEST,
        "a non-Query body type must be rejected: {:?}",
        response.body
    );
}

/// R2-92: percent-decoding at the presentation boundary is targeted -- a
/// Relationship's `object` (a percent-encoded entity id) is decoded, while a
/// Property value carrying a literal `%` is left untouched. The notification
/// worker now shares this one walk, so a notification decodes as a GET does.
#[tokio::test]
async fn regression_r2_92_relationship_object_decodes_but_property_percent_survives() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let target = scope.id("Building:r2-92-target");
    let id = scope.id("Device:r2-92");
    let encoded_target = target.replace(':', "%3A");
    let entity = json!({
        "id": id,
        "type": "Device",
        "ref": {"type": "Relationship", "object": encoded_target},
        "ratio": {"type": "Property", "value": "50%"},
        "@context": [TEST_SUITE_CONTEXT]
    });
    assert_eq!(client.post_entity(&entity).send().await.expect("create").status, StatusCode::CREATED);

    let response = client.get_entity(&id, "", "application/json", Some(TEST_SUITE_CONTEXT)).await.expect("get");
    assert_eq!(response.status, StatusCode::OK);
    let response_body = response.json_value().expect("retrieved entity must be valid JSON");
    assert_eq!(response_body["ref"]["object"], target.as_str(), "the relationship target must be decoded");
    assert_eq!(response_body["ratio"]["value"], "50%", "a literal property percent must survive");
    scope.finish().await.expect("cleanup failed");
}

/// R2-136: a `type` selector that is not a clause 4.6.2 name is rejected at the
/// query seam. The leaf of a type expression is now an `EntityTypeName`, so a
/// value like `42bad` is a `BadRequestData` rather than a leaf parsed to match
/// nothing.
#[tokio::test]
async fn regression_r2_136_a_malformed_type_selector_is_rejected() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let response = client
        .query_entities("type=42bad", "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query entities request failed");
    assert_eq!(
        response.status,
        StatusCode::BAD_REQUEST,
        "a type name outside the clause 4.6.2 grammar must be rejected: {:?}",
        response.body
    );
}

/// R2-144: every identifier is admitted through the one lexicon `Uri` parser
/// (a URN or a URL), so a malformed registration id is rejected while a
/// well-formed URN parses and is merely absent.
#[tokio::test]
async fn regression_r2_144_a_malformed_id_is_rejected_and_a_bare_urn_parses() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let malformed = client
        .raw(Method::GET, "/ngsi-ld/v1/csourceRegistrations/not-a-uri")
        .send()
        .await
        .expect("malformed id request failed");
    assert_eq!(
        malformed.status,
        StatusCode::BAD_REQUEST,
        "an identifier that is neither a URN nor a URL must be rejected: {:?}",
        malformed.body
    );

    let absent = client
        .raw(
            Method::GET,
            "/ngsi-ld/v1/csourceRegistrations/urn:ngsi-ld:ContextSourceRegistration:r2-144-absent",
        )
        .send()
        .await
        .expect("urn id request failed");
    assert_eq!(
        absent.status,
        StatusCode::NOT_FOUND,
        "a well-formed URN id parses and is merely absent, not malformed: {:?}",
        absent.body
    );
}

/// R2-74: with the doubled-prefix rewrite shim gone, a hosted `@context`
/// registered under its canonical `/ngsi-ld/v1/jsonldContexts/{id}` URL still
/// resolves through the registry — registration and retrieval round-trip.
#[tokio::test]
async fn regression_r2_74_a_hosted_context_resolves_via_its_canonical_registry_key() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let payload = json!({ "@context": { "r2_74_term": "https://example.com/r2-74/term" } });
    let created = client.create_jsonld_context(&payload).await.expect("create context request failed");
    assert_eq!(created.status, StatusCode::CREATED, "hosted context creation must succeed: {:?}", created.body);

    let location = created
        .location()
        .expect("Location header must contain valid text")
        .expect("a Location header naming the hosted context");
    let id = location.rsplit('/').next().expect("the Location names a context id");

    let retrieved = client.retrieve_jsonld_context(id).await.expect("retrieve context request failed");
    assert_eq!(
        retrieved.status,
        StatusCode::OK,
        "the hosted context must resolve via its canonical registry key: {:?}",
        retrieved.body
    );
}
