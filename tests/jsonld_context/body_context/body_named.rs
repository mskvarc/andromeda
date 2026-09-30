//! Request contexts only an `application/ld+json` body can name (study A5 to
//! A7, A10 and A11).
//!
//! A `Link` names one URL; an array of contexts, an inline definition or an
//! `@import` can only travel in a body. The POST query operations (clause 5.7.2
//! over clause 6.23 and its temporal twin, clause 6.24) carry one and answer
//! every Entity with the rule's value.

use crate::{
    broker_endpoints::broker_url,
    core_contexts::{CORE_V13, CORE_V16, CORE_V18, core, core_value},
    ngsi_requests::{create_device, ok_body},
    private_tenant::PrivateTenant,
    unique_ids::unique_id,
    user_contexts::{DEVICE_IRI, UserContexts, term_definitions},
};
use reqwest::Client;
use serde_json::{Value, json};

/// The `@context` every Entity of an `application/ld+json` entity and temporal
/// POST query naming `context` renders, per operation.
async fn rendered_by_queries(client: &Client, device: &str, context: &Value) -> Vec<(&'static str, Value)> {
    let entities = json!([{ "type": DEVICE_IRI, "id": device }]);
    let mut rendered = Vec::new();
    for (operation, path, query) in [
        ("entity POST query", "entityOperations/query", json!({ "type": "Query", "entities": entities })),
        (
            "temporal POST query",
            "temporal/entityOperations/query",
            json!({
                "type": "Query",
                "entities": entities,
                "temporalQ": { "timerel": "after", "timeAt": "2000-01-01T00:00:00Z", "timeproperty": "modifiedAt" }
            }),
        ),
    ] {
        let mut body = query;
        body["@context"] = context.clone(); // clone: each operation's body owns the context
        let response = client
            .post(broker_url(path))
            .header("Content-Type", "application/ld+json")
            .header("Accept", "application/ld+json")
            .json(&body)
            .send()
            .await
            .expect("POST query failed");
        let answered = ok_body(response).await;
        let answered_entities = answered.as_array().expect("a query answers an array");
        assert_eq!(answered_entities.len(), 1, "{operation} finds the device: {answered}");
        rendered.push((operation, answered_entities[0]["@context"].clone())); // clone: the answer is dropped after this
    }
    rendered
}

/// Create a device under the served `terms` and assert that both POST queries
/// naming `context` render `expected` on every Entity.
async fn assert_rendered(context: fn(&UserContexts) -> Value, expected: fn(&UserContexts) -> Value) {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let device = unique_id("urn:ngsi-ld:Device:body-named");
    create_device(tenant.client(), &device, &contexts.terms).await;

    let rendered = rendered_by_queries(tenant.client(), &device, &context(&contexts)).await;

    let expected = expected(&contexts);
    for (operation, named) in rendered {
        assert_eq!(named, expected, "{operation}");
    }
    contexts.shutdown().await;
    tenant.finish().await;
}

/// Study A5: an inline definition without the core `@context` is followed by
/// it.
#[tokio::test]
async fn an_inline_definition_without_the_core_context_is_followed_by_it() {
    assert_rendered(|_| term_definitions(), |_| json!([term_definitions(), core()])).await;
}

/// Study A6: an older core release named after a user context is removed, and
/// v1.9 is named last.
#[tokio::test]
async fn an_older_core_release_after_a_user_context_is_replaced_by_the_current_one_last() {
    assert_rendered(|contexts| json!([contexts.terms, CORE_V16]), |contexts| json!([contexts.terms, core()])).await;
}

/// Study A7: an older core release named before a user context is removed,
/// and v1.9 is named last.
#[tokio::test]
async fn an_older_core_release_before_a_user_context_is_moved_last_as_the_current_one() {
    assert_rendered(|contexts| json!([CORE_V18, contexts.terms]), |contexts| json!([contexts.terms, core()])).await;
}

/// Study A10: a context that ends with the core `@context`, followed by the
/// core `@context` itself, keeps the core `@context` named last once.
#[tokio::test]
async fn a_compound_context_followed_by_the_core_context_keeps_it_last() {
    assert_rendered(|contexts| json!([contexts.wrapper, core()]), |contexts| json!([contexts.wrapper, core()])).await;
}

/// Study A11 and JSON-LD 1.1 section 4.1.10: an inline definition importing
/// the protected v1.8 core release beside its own terms is processed without a
/// protected term redefinition, and renders its own terms followed by v1.9.
#[tokio::test]
async fn an_inline_import_of_an_older_core_release_is_replaced_by_the_current_one_last() {
    assert_rendered(
        |_| {
            let mut definition = term_definitions();
            definition["@import"] = json!(CORE_V18);
            definition
        },
        |_| json!([term_definitions(), core()]),
    )
    .await;
}

/// Study A11: an inline definition that only imports an older core release
/// leaves nothing of its own, so the answer names v1.9 alone.
#[tokio::test]
async fn an_inline_import_of_an_older_core_release_alone_is_answered_as_the_current_one() {
    assert_rendered(|_| json!({ "@import": CORE_V13 }), |_| core_value()).await;
}

/// The core `@context` named twice, directly, is named once.
#[tokio::test]
async fn the_core_context_named_twice_is_named_once() {
    assert_rendered(|contexts| json!([core(), contexts.terms, core()]), |contexts| json!([contexts.terms, core()])).await;
}
