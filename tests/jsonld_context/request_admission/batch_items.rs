//! Each item of an `application/ld+json` batch is expanded under its own
//! `@context`.
//!
//! In JSON-LD each top-level node's `@context` applies to that node; a batch of
//! `[{..., "@context": A}, {..., "@context": B}]` expands item 1 under A and
//! item 2 under B, and a union would let B's definitions redefine A's terms for
//! item 1.

use crate::{
    broker_endpoints::broker_url,
    context_origin::ContextOrigin,
    core_contexts::core,
    ngsi_requests::stored_entity,
    private_tenant::PrivateTenant,
    request_admission::speed_context,
    unique_ids::unique_id,
};
use reqwest::StatusCode;
use serde_json::{Value, json};

/// A batch item naming `speed` through an inline context mapping it to `iri`.
fn inline_item(id: &str, iri: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": { "type": "Property", "value": 1 },
        "@context": [{ "speed": iri }, core()]
    })
}

/// Items naming `speed` through different inline contexts store different
/// attribute IRIs, and neither reads the other's definition, on create and
/// on upsert.
#[tokio::test]
async fn each_item_of_a_batch_is_expanded_under_its_own_inline_context() {
    let tenant = PrivateTenant::provisioned().await;

    for operation in ["create", "upsert"] {
        let first = unique_id("urn:ngsi-ld:Vehicle:batch-a");
        let second = unique_id("urn:ngsi-ld:Vehicle:batch-b");
        let third = unique_id("urn:ngsi-ld:Vehicle:batch-c");
        let response = tenant
            .client()
            .post(broker_url(&format!("entityOperations/{operation}")))
            .header("Content-Type", "application/ld+json")
            .json(&json!([
                inline_item(&first, "https://a.example/speed"),
                inline_item(&second, "https://b.example/speed"),
                inline_item(&third, "https://a.example/speed")
            ]))
            .send()
            .await
            .expect("batch request failed");
        assert!(response.status().is_success(), "{operation}: {:?}", response.text().await);

        for (id, own, other) in [
            (&first, "https://a.example/speed", "https://b.example/speed"),
            (&second, "https://b.example/speed", "https://a.example/speed"),
            (&third, "https://a.example/speed", "https://b.example/speed"),
        ] {
            let stored = stored_entity(tenant.client(), id).await;
            assert!(stored.get(own).is_some(), "{operation}: {id} stores its own IRI: {stored}");
            assert!(stored.get(other).is_none(), "{operation}: {id} never reads the other item's context: {stored}");
        }
    }
    tenant.finish().await;
}

/// Items naming two different served contexts are each expanded through
/// their own.
#[tokio::test]
async fn batch_items_naming_different_served_contexts_are_each_expanded_through_their_own() {
    let tenant = PrivateTenant::provisioned().await;
    let origin = ContextOrigin::start().await;
    let a = origin.serve_document("/batch/a.jsonld", &speed_context("https://a.example/speed")).await;
    let b = origin.serve_document("/batch/b.jsonld", &speed_context("https://b.example/speed")).await;
    let first = unique_id("urn:ngsi-ld:Vehicle:served-a");
    let second = unique_id("urn:ngsi-ld:Vehicle:served-b");

    let created = tenant
        .client()
        .post(broker_url("entityOperations/create"))
        .header("Content-Type", "application/ld+json")
        .json(&json!([
            { "id": first, "type": "Vehicle", "speed": { "type": "Property", "value": 1 }, "@context": a },
            { "id": second, "type": "Vehicle", "speed": { "type": "Property", "value": 2 }, "@context": b }
        ]))
        .send()
        .await
        .expect("batch create failed");

    assert_eq!(created.status(), StatusCode::CREATED, "{:?}", created.text().await);
    for (id, own, value) in [(&first, "https://a.example/speed", 1), (&second, "https://b.example/speed", 2)] {
        let stored = stored_entity(tenant.client(), id).await;
        assert_eq!(stored[own]["value"], json!(value), "{id} stores its own IRI: {stored}");
    }
    origin.shutdown().await;
    tenant.finish().await;
}
