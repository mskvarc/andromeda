//! A context the request names twice (study A17).
//!
//! JSON-LD processes an `@context` array in order, and a top-level context
//! named again is processed again (JSON-LD 1.1 API section 4.1.2 step 5.2.2
//! skips a repeat only while a scoped context is validated), so `[A, B, A]`
//! leaves A's definitions in force where `[A, B]` leaves B's. Clause 5.5.7 has
//! the broker use and name the context the call provided, repeats included.

use crate::{
    broker_endpoints::broker_url,
    core_contexts::core,
    ngsi_requests::{ok_body, stored_entity},
    private_tenant::PrivateTenant,
    unique_ids::unique_id,
    user_contexts::{OTHER_SPEED_IRI, SPEED_IRI, UserContexts},
};
use reqwest::StatusCode;
use serde_json::json;

/// `[A, B, A]` is rendered with its repeat, and the term both define expands to
/// A's IRI, the definition processed last.
#[tokio::test]
async fn a_context_named_again_after_another_is_rendered_and_processed_again() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let repeated = json!([contexts.terms, contexts.second, contexts.terms]);
    let id = unique_id("urn:ngsi-ld:Device:repeated");

    let created = tenant
        .client()
        .post(broker_url("entities"))
        .header("Content-Type", "application/ld+json")
        .json(&json!({ "id": id, "type": "Device", "speed": { "type": "Property", "value": 4 }, "@context": repeated }))
        .send()
        .await
        .expect("create request failed");
    assert_eq!(created.status(), StatusCode::CREATED, "{:?}", created.text().await);

    let rendered = ok_body(
        tenant
            .client()
            .post(broker_url("entityOperations/query"))
            .header("Content-Type", "application/ld+json")
            .header("Accept", "application/ld+json")
            .json(&json!({ "type": "Query", "entities": [{ "type": "Device", "id": id }], "@context": repeated }))
            .send()
            .await
            .expect("POST query failed"),
    )
    .await;
    assert_eq!(
        rendered[0]["@context"],
        json!([contexts.terms, contexts.second, contexts.terms, core()]),
        "the repeat is named: {rendered}"
    );
    let stored = stored_entity(tenant.client(), &id).await;
    assert_eq!(stored[SPEED_IRI]["value"], json!(4), "A's definition is in force last: {stored}");
    assert!(stored.get(OTHER_SPEED_IRI).is_none(), "B's definition was overridden: {stored}");
    contexts.shutdown().await;
    tenant.finish().await;
}
