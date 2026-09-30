//! Which carrier a request's context may take for its media type (study C6
//! and C10).

use crate::{
    broker_endpoints::broker_url,
    context_links::{context_link, profile_link},
    core_contexts::core,
    private_tenant::PrivateTenant,
    request_admission::assert_bad_request_data,
    unique_ids::unique_id,
};
use reqwest::StatusCode;
use serde_json::{Value, json};

/// A `Vehicle` body naming the v1.9 core context inline.
fn ld_vehicle(id: &str) -> Value {
    json!({ "id": id, "type": "Vehicle", "@context": core() })
}

/// Study C6: an `application/ld+json` body beside a context `Link` is refused,
/// for a single entity and for a batch, and nothing is created; beside a
/// `profile` link alone it is admitted.
#[tokio::test]
async fn an_ld_json_body_refuses_a_context_link_and_admits_a_profile_link() {
    let tenant = PrivateTenant::provisioned().await;
    let refused_id = unique_id("urn:ngsi-ld:Vehicle:ld-link");

    let refused = tenant
        .client()
        .post(broker_url("entities"))
        .header("Content-Type", "application/ld+json")
        .header("Link", context_link(core()))
        .json(&ld_vehicle(&refused_id));
    assert_bad_request_data(refused, "ld+json with a context Link").await;
    let absent = tenant
        .client()
        .get(broker_url(&format!("entities/{refused_id}")))
        .send()
        .await
        .expect("retrieve failed");
    assert_eq!(absent.status(), StatusCode::NOT_FOUND, "the refused entity was not created");

    let batch_refused = tenant
        .client()
        .post(broker_url("entityOperations/create"))
        .header("Content-Type", "application/ld+json")
        .header("Link", context_link(core()))
        .json(&json!([ld_vehicle(&unique_id("urn:ngsi-ld:Vehicle:ld-batch"))]));
    assert_bad_request_data(batch_refused, "an ld+json batch with a context Link").await;

    let admitted = tenant
        .client()
        .post(broker_url("entities"))
        .header("Content-Type", "application/ld+json")
        .header("Link", profile_link())
        .json(&ld_vehicle(&unique_id("urn:ngsi-ld:Vehicle:ld-profile")))
        .send()
        .await
        .expect("create failed");
    assert_eq!(admitted.status(), StatusCode::CREATED);
    tenant.finish().await;
}

/// Study C10: clause 6.3.5 refuses an `application/json` body carrying
/// `@context` and an `application/ld+json` body carrying none.
#[tokio::test]
async fn a_context_in_the_wrong_body_or_none_in_the_right_one_is_refused() {
    let tenant = PrivateTenant::provisioned().await;

    let json_with_context = tenant
        .client()
        .post(broker_url("entities"))
        .header("Content-Type", "application/json")
        .json(&ld_vehicle(&unique_id("urn:ngsi-ld:Vehicle:json-ctx")));
    assert_bad_request_data(json_with_context, "application/json with @context").await;

    let ld_without_context = tenant
        .client()
        .post(broker_url("entities"))
        .header("Content-Type", "application/ld+json")
        .json(&json!({ "id": unique_id("urn:ngsi-ld:Vehicle:ld-no-ctx"), "type": "Vehicle" }));
    assert_bad_request_data(ld_without_context, "application/ld+json without @context").await;
    tenant.finish().await;
}
