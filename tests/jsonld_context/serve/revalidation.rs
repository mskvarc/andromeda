//! Revalidating a stored `@context` across broker processes.
//!
//! A Hosted `@context` has no origin server to make it stale, but another
//! broker process on the same database can delete it. A process that used the
//! document keeps its copy only for the revalidation span the Serve response
//! advertises as `max-age`, then reads the row again, and so stops resolving a
//! document another process deleted.

use crate::{
    broker_endpoints::{broker_url, second_broker_url},
    context_api::{add_hosted, delete_context, serve},
    polling::eventually,
    private_tenant::PrivateTenant,
    problem_types::LD_CONTEXT_NOT_AVAILABLE_TYPE,
    unique_ids::unique_id,
};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};

/// Create an entity on the second broker under `context`: the status and the
/// body.
async fn create_on_second_broker(client: &Client, context: &str) -> (StatusCode, Value) {
    let response = client
        .post(second_broker_url("entities"))
        .header("Content-Type", "application/ld+json")
        .json(&json!({
            "id": unique_id("urn:ngsi-ld:Spot:revalidation"),
            "type": "Spot",
            "capacity": { "type": "Property", "value": 1 },
            "@context": [context]
        }))
        .send()
        .await
        .expect("create request to the second broker failed");
    let status = response.status();
    (status, response.json().await.unwrap_or_default())
}

/// A document deleted through the first broker stops resolving on the second
/// once the second broker's copy has lapsed, instead of resolving there for
/// the life of that process.
#[tokio::test]
async fn a_hosted_context_deleted_on_one_broker_stops_resolving_on_another_after_the_revalidation_span() {
    let tenant = PrivateTenant::provisioned().await;
    let id = add_hosted(tenant.client(), &json!({ "@context": { "capacity": "https://e.org/capacity" } })).await;
    let serving_url = broker_url(&format!("jsonldContexts/{id}"));
    let span = serve(tenant.client(), Method::GET, &id, &[]).await.max_age().expect("a max-age");
    assert!(span > 0, "a positive revalidation span");

    let (resolved, body) = create_on_second_broker(tenant.client(), &serving_url).await;
    assert_eq!(resolved, StatusCode::CREATED, "the second broker resolves the stored document: {body}");

    let (deleted, _) = delete_context(tenant.client(), &id, "").await;
    assert_eq!(deleted, StatusCode::NO_CONTENT);

    let client = tenant.client();
    let context = &serving_url;
    let problem = eventually("the second broker stops resolving the deleted document", move || async move {
        let (status, problem) = create_on_second_broker(client, context).await;
        (!status.is_success()).then_some(problem)
    })
    .await;
    assert_eq!(problem["type"], LD_CONTEXT_NOT_AVAILABLE_TYPE.as_str(), "{problem}");
    tenant.finish().await;
}
