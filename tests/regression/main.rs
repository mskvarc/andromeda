//! Regression tests for defects that do not belong to any feature suite.
//!
//! The catch-all target named in the batch-3 fix plan: each test reproduces a
//! specific review finding through the HTTP API. Naming convention:
//! `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use andromeda_test_common::prelude::*;
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
    let scope = TestScope::new("regression");
    let client = NgsiLdClient::new(&scope).await;
    let receiver = NotificationReceiver::start().await;

    let subscription_id = scope.id("Subscription:r2-50-receiverinfo");
    let subscription = json!({
        "id": subscription_id,
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "notification": {
            "endpoint": {
                "uri": receiver.endpoint_url(),
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

    let captured = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(15)).await;
    let authorization = captured
        .headers
        .get("authorization")
        .expect("the receiverInfo Authorization header must reach the endpoint, not be silently dropped");
    assert_eq!(authorization.to_str().expect("ascii header"), "Bearer secret-token-r2-50");

    scope.cleanup().await.expect("cleanup failed");
}

/// R2-113: a JSON-LD context `Link` header whose parameters carry an escaped
/// quote (RFC 8288 quoted-pair) and a semicolon inside the quotes must still be
/// parsed correctly, so the context is applied and the entity's compact `type`
/// resolves. A parser that toggled quote state on the escaped quote severed the
/// value and could shadow the real `rel`, losing the context.
#[tokio::test]
async fn regression_r2_113_context_link_with_an_escaped_quote_still_resolves_the_context() {
    let scope = TestScope::new("regression");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:r2-113");

    // A `title` parameter carrying an escaped quote and a semicolon inside its
    // quotes, placed before the real `rel`. A parser that ended the quoted value
    // at the escaped quote would split on the inner `;` and expose a spurious
    // `rel`, hiding the genuine context link.
    let link = format!(
        "<{context}>; title=\"a\\\"b; rel=nonsense\"; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"",
        context = TEST_SUITE_CONTEXT
    );

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
        .header("Link", &jsonld_link_header(TEST_SUITE_CONTEXT))
        .send()
        .await
        .expect("retrieve entity failed");
    assert_eq!(retrieved.status, StatusCode::OK);
    assert_eq!(
        retrieved.body["type"], "Building",
        "the Link context must have been applied on write: {:?}",
        retrieved.body
    );

    scope.cleanup().await.expect("cleanup failed");
}

/// R2-24: a POST query body's `entities[]` is a union of independent selectors
/// (clause 5.2.11), not a flattened cross product. `{id:A,type:T1}` OR
/// `{id:B,type:T2}` must not match an entity `(id:A, type:T2)` that pairs one
/// selector's id with another's type -- the pairing the old flattened
/// `id IN (A,B) AND type IN (T1,T2)` form wrongly admitted.
#[tokio::test]
async fn regression_r2_24_post_query_selectors_are_a_union_not_a_cross_product() {
    let scope = TestScope::new("regression");
    let client = NgsiLdClient::new(&scope).await;

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
        .raw(Method::POST, "/ngsi-ld/v1/entities/query")
        .header("Link", &jsonld_link_header(TEST_SUITE_CONTEXT))
        .json(&query_body)
        .send()
        .await
        .expect("post query failed");
    assert_eq!(response.status, StatusCode::OK, "query: {:?}", response.body);

    let returned: Vec<String> = response
        .body
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

    scope.cleanup().await.expect("cleanup failed");
}

/// R2-55: clause 4.19 admits a conjunction only parenthesised
/// (`OrScopeQ = "(" ScopeQ *(andOp ScopeQ) ")" / ScopeQ`). A subscription that
/// stores a `scopeQ` conjunction `/A;/B` must render it back as `(/A;/B)` -- text
/// a conforming peer broker accepts -- not the bare `/A;/B` the renderer used to
/// emit.
#[tokio::test]
async fn regression_r2_55_stored_scope_query_conjunction_renders_parenthesised() {
    let scope = TestScope::new("regression");
    let client = NgsiLdClient::new(&scope).await;

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
    assert_eq!(
        retrieved.body["scopeQ"], "(/Madrid;/Barcelona)",
        "a stored scopeQ conjunction must render parenthesised (clause 4.19): {:?}",
        retrieved.body["scopeQ"]
    );

    scope.cleanup().await.expect("cleanup failed");
}
