//! A request context whose remote references never end, or end too deep, is
//! refused, and refusing it leaves the broker serving (study A14, as ruled).
//!
//! JSON-LD 1.1 API section 4.1.2 step 5.2.2 bounds the remote contexts one
//! processing run may follow and raises a "context overflow" error past the
//! bound, which is the only way a reference cycle can end. Clause 5.5.6 makes a
//! request `@context` that cannot be processed `BadRequestData`. The ruling
//! applies the same answer to a cycle through the context array, a document
//! naming itself, a cycle through `@import`, and a chain one document longer
//! than the broker follows; a chain exactly as long as it follows is admitted
//! like any other. After every refusal the same broker answers an ordinary
//! request, so a refused context never takes the process down.

use crate::{
    broker_endpoints::broker_url,
    context_api::implicit_context_ids,
    context_links::context_link,
    context_origin::ContextOrigin,
    core_contexts::core,
    ngsi_requests::{create_device, get, ok_body, stored_entity},
    private_tenant::PrivateTenant,
    request_admission::{assert_bad_request_data, requests_for_every_verb},
    unique_ids::unique_id,
    user_contexts::{DEVICE_IRI, SPEED_IRI, UserContexts, term_definitions},
};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};

/// How many documents one reference path may hold, counting the one the request
/// names as the first and every string or `@import` reference as one more (core
/// `@context` references do not count): a `Link` naming the first of this many
/// documents, each naming the next, is admitted, and one more is refused.
const REFERENCE_DEPTH_LIMIT: usize = 8;

/// Serve a chain of `length` documents under `prefix`, each naming the next and
/// the last defining the two terms before the v1.9 core `@context`, and return
/// the URL of the first.
async fn serve_chain(origin: &ContextOrigin, prefix: &str, length: usize) -> String {
    let mut next = origin
        .serve_document(&format!("/{prefix}/{length}.jsonld"), &json!({ "@context": [term_definitions(), core()] }))
        .await;
    for position in (1..length).rev() {
        next = origin
            .serve_document(&format!("/{prefix}/{position}.jsonld"), &json!({ "@context": next }))
            .await;
    }
    next
}

/// Assert the broker still answers an ordinary retrieval of `device` under the
/// served `terms`, after `what` was refused.
async fn assert_still_serving(client: &Client, device: &str, terms: &str, what: &str) {
    let answered = get(client, &format!("entities/{device}"), "application/ld+json", Some(terms)).await;
    assert_eq!(answered.status(), StatusCode::OK, "the broker still serves after {what}");
}

/// Refuse `context`, named in a `Link` on every verb that expands terms, and in
/// an `application/ld+json` create and POST query body; nothing is written, no
/// wrapper is hosted, and the broker keeps serving.
async fn assert_refused_everywhere(client: &Client, contexts: &UserContexts, context: &str, what: &str) {
    let device = unique_id("urn:ngsi-ld:Device:reference-chain");
    create_device(client, &device, &contexts.terms).await;

    // A DELETE by identifier names no term, so the broker never loads its
    // request context (an unreachable one is not refused either): only the
    // verbs that expand terms read, and so refuse, the context.
    for (verb, request) in requests_for_every_verb(client, &device).into_iter().filter(|(verb, _)| *verb != "DELETE") {
        assert_bad_request_data(request.header("Link", context_link(context)), &format!("{verb} naming {what}")).await;
        assert_still_serving(client, &device, &contexts.terms, &format!("{verb} naming {what}")).await;
    }
    let created = unique_id("urn:ngsi-ld:Device:reference-chain-body");
    assert_bad_request_data(
        client
            .post(broker_url("entities"))
            .header("Content-Type", "application/ld+json")
            .json(&json!({ "id": created, "type": "Device", "@context": [context] })),
        &format!("an ld+json create naming {what}"),
    )
    .await;
    assert_bad_request_data(
        client
            .post(broker_url("entityOperations/query"))
            .header("Content-Type", "application/ld+json")
            .header("Accept", "application/json")
            .json(&json!({ "type": "Query", "entities": [{ "type": DEVICE_IRI }], "@context": context })),
        &format!("an ld+json POST query naming {what}"),
    )
    .await;
    assert_still_serving(client, &device, &contexts.terms, &format!("the bodies naming {what}")).await;

    let stored = stored_entity(client, &device).await;
    assert_eq!(stored[SPEED_IRI]["value"], json!(3), "the refused PATCH changed nothing: {stored}");
    let missing = get(client, &format!("entities/{created}"), "application/json", None).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND, "the refused create wrote nothing");
    assert!(implicit_context_ids(client).await.is_empty(), "no wrapper was hosted for {what}");
}

/// Study A14 as ruled: `a` naming `b` naming `a` is refused wherever a request
/// names it.
#[tokio::test]
async fn a_reference_cycle_is_refused_and_the_broker_keeps_serving() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let first = contexts.origin().url("/cycle/a.jsonld");
    let second = contexts.origin().serve_document("/cycle/b.jsonld", &json!({ "@context": first })).await;
    contexts
        .origin()
        .serve_document("/cycle/a.jsonld", &json!({ "@context": [contexts.terms, second] }))
        .await;

    assert_refused_everywhere(tenant.client(), &contexts, &first, "a reference cycle").await;
    contexts.shutdown().await;
    tenant.finish().await;
}

/// A document whose `@context` names the document itself is a cycle of one.
#[tokio::test]
async fn a_self_referencing_context_is_refused_and_the_broker_keeps_serving() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let itself = contexts.origin().url("/cycle/itself.jsonld");
    contexts
        .origin()
        .serve_document("/cycle/itself.jsonld", &json!({ "@context": [term_definitions(), itself] }))
        .await;

    assert_refused_everywhere(tenant.client(), &contexts, &itself, "a self-referencing context").await;
    contexts.shutdown().await;
    tenant.finish().await;
}

/// JSON-LD 1.1 API section 4.1.2 step 5.6 loads an `@import` target as a remote
/// context too, so two definitions importing each other are a cycle.
#[tokio::test]
async fn an_import_cycle_is_refused_and_the_broker_keeps_serving() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let first = contexts.origin().url("/import-cycle/a.jsonld");
    let second = contexts
        .origin()
        .serve_document(
            "/import-cycle/b.jsonld",
            &json!({ "@context": { "@import": first, "rate": "https://example.org/terms/rate" } }),
        )
        .await;
    let mut importing = term_definitions();
    importing["@import"] = json!(second);
    contexts
        .origin()
        .serve_document("/import-cycle/a.jsonld", &json!({ "@context": importing }))
        .await;

    assert_refused_everywhere(tenant.client(), &contexts, &first, "an @import cycle").await;
    contexts.shutdown().await;
    tenant.finish().await;
}

/// A chain exactly as long as the broker follows is an ordinary context: it
/// ends with the core `@context`, so it is named alone, and its terms expand.
#[tokio::test]
async fn a_chain_at_the_depth_limit_is_admitted_and_named_alone() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let head = serve_chain(contexts.origin(), "at-limit", REFERENCE_DEPTH_LIMIT).await;
    let device = unique_id("urn:ngsi-ld:Device:at-limit");

    create_device(tenant.client(), &device, &head).await;
    let rendered = ok_body(get(tenant.client(), &format!("entities/{device}"), "application/ld+json", Some(&head)).await).await;

    assert_eq!(rendered["@context"], json!(head), "{rendered}");
    assert_eq!(rendered["type"], json!("Device"), "the chain's terms compact the type: {rendered}");
    let stored = stored_entity(tenant.client(), &device).await;
    assert_eq!(stored["type"], json!(DEVICE_IRI), "{stored}");
    contexts.shutdown().await;
    tenant.finish().await;
}

/// One document more than the broker follows is refused like a cycle, and the
/// document past the bound is never fetched (step 5.2.2 checks the bound before
/// dereferencing).
#[tokio::test]
async fn a_chain_one_past_the_depth_limit_is_refused_and_the_broker_keeps_serving() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let head = serve_chain(contexts.origin(), "past-limit", REFERENCE_DEPTH_LIMIT + 1).await;

    assert_refused_everywhere(tenant.client(), &contexts, &head, "a chain past the depth limit").await;
    let past = format!("/past-limit/{}.jsonld", REFERENCE_DEPTH_LIMIT + 1);
    assert_eq!(contexts.origin().fetch_count(&past), 0, "the document past the limit is never fetched");
    contexts.shutdown().await;
    tenant.finish().await;
}

/// The refusal is the same for a Subscription whose `jsonldContext` is a
/// cycle: nothing is stored.
#[tokio::test]
async fn a_subscription_naming_a_reference_cycle_is_refused() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let first = contexts.origin().url("/cycle/a.jsonld");
    let second = contexts.origin().serve_document("/cycle/b.jsonld", &json!({ "@context": first })).await;
    contexts
        .origin()
        .serve_document("/cycle/a.jsonld", &json!({ "@context": [contexts.terms, second] }))
        .await;

    assert_bad_request_data(
        tenant
            .client()
            .post(broker_url("subscriptions"))
            .header("Content-Type", "application/ld+json")
            .json(&json!({
                "type": "Subscription",
                "entities": [{ "type": "Device" }],
                "notification": { "endpoint": { "uri": "http://127.0.0.1:1/notify", "accept": "application/json" } },
                "@context": [first]
            })),
        "a subscription naming a reference cycle",
    )
    .await;
    let listed: Value = ok_body(tenant.client().get(broker_url("subscriptions")).send().await.expect("list failed")).await;
    assert_eq!(listed, json!([]), "no subscription was stored: {listed}");
    contexts.shutdown().await;
    tenant.finish().await;
}
