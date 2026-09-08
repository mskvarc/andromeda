//! Registry API response codes for `csourceSubscriptions` and the source-identity
//! resource (clause 6.11, 6.33): the `201`/`Location` create, the `400`/`409`
//! create rejections, the array query, the retrieve/update/delete
//! `200`/`204`/`400`/`404` outcomes, and the `200`/`501` source identity. All
//! `T-LOCAL`.

use crate::{
    http::forwarding::{PLACEHOLDER_ENDPOINT, context_link, context_link_header},
    oracle::{assert_status, json_body, location},
    topology::{self, BrokerRole},
};
use andromeda_test_client::response::TestResponse;
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use reqwest::Method;
use serde_json::{Value, json};

/// A csource subscription over `Vehicle` registrations delivering to a
/// placeholder endpoint (never contacted by these lifecycle assertions).
fn subscription_body(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [ { "type": "Vehicle" } ],
        "notification": { "endpoint": { "uri": PLACEHOLDER_ENDPOINT, "accept": "application/json" } },
        "@context": context_link()
    })
}

/// POST a csource subscription body to the front broker.
async fn create_sub(topology: &MultiBrokerTopology<BrokerRole>, body: &Value) -> TestResponse {
    topology
        .broker(BrokerRole::A)
        .client()
        .raw(Method::POST, "/ngsi-ld/v1/csourceSubscriptions")
        .header("content-type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create csource subscription")
}

/// GET one csource subscription by id.
async fn get_sub(topology: &MultiBrokerTopology<BrokerRole>, id: &str) -> TestResponse {
    topology
        .broker(BrokerRole::A)
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/csourceSubscriptions/{}", urlencoding::encode(id)))
        .header("accept", "application/ld+json")
        .header("link", context_link_header())
        .send()
        .await
        .expect("retrieve csource subscription")
}

/// PATCH one csource subscription by id.
async fn patch_sub(topology: &MultiBrokerTopology<BrokerRole>, id: &str, fragment: &Value) -> TestResponse {
    topology
        .broker(BrokerRole::A)
        .client()
        .raw(Method::PATCH, &format!("/ngsi-ld/v1/csourceSubscriptions/{}", urlencoding::encode(id)))
        .header("content-type", "application/json")
        .header("link", context_link_header())
        .json(fragment)
        .send()
        .await
        .expect("update csource subscription")
}

/// DELETE one csource subscription by id.
async fn delete_sub(topology: &MultiBrokerTopology<BrokerRole>, id: &str) -> TestResponse {
    topology
        .broker(BrokerRole::A)
        .client()
        .raw(Method::DELETE, &format!("/ngsi-ld/v1/csourceSubscriptions/{}", urlencoding::encode(id)))
        .send()
        .await
        .expect("delete csource subscription")
}

/// A valid csource subscription create returns `201` with a `Location`
/// (clause 6.11.3.1). Confirms REQ-HTTP-86.
#[tokio::test]
async fn dist_http_create_csource_subscription_returns_201_location() {
    let topology = topology::single().await.expect("single-broker topology");
    let sub_id = topology.id("Subscription:cssub-201");
    let response = create_sub(&topology, &subscription_body(&sub_id)).await;
    assert_status(&response, 201);
    assert!(location(&response).ends_with(&sub_id), "the Location names the created subscription");
    let _cleanup = delete_sub(&topology, &sub_id).await;
    topology.finish().await.expect("topology cleanup");
}

/// An invalid csource subscription body is rejected with `400` (clause 6.11.3.1).
/// Confirms REQ-HTTP-87.
#[tokio::test]
async fn dist_http_create_csource_subscription_invalid_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = create_sub(&topology, &json!({ "type": "Subscription", "@context": context_link() })).await;
    assert_status(&response, 400);
    topology.finish().await.expect("topology cleanup");
}

/// Re-creating an existing csource subscription is rejected with `409`
/// (clause 6.11.3.1). Confirms REQ-HTTP-87.
#[tokio::test]
async fn dist_http_create_csource_subscription_duplicate_returns_409() {
    let topology = topology::single().await.expect("single-broker topology");
    let sub_id = topology.id("Subscription:cssub-409");
    assert_status(&create_sub(&topology, &subscription_body(&sub_id)).await, 201);
    assert_status(&create_sub(&topology, &subscription_body(&sub_id)).await, 409);
    let _cleanup = delete_sub(&topology, &sub_id).await;
    topology.finish().await.expect("topology cleanup");
}

/// A csource subscription query returns `200` with an array body (clause
/// 6.11.3.2). Confirms REQ-HTTP-88.
#[tokio::test]
async fn dist_http_query_csource_subscriptions_returns_200_array() {
    let topology = topology::single().await.expect("single-broker topology");
    let sub_id = topology.id("Subscription:cssub-array");
    assert_status(&create_sub(&topology, &subscription_body(&sub_id)).await, 201);

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/csourceSubscriptions")
        .header("accept", "application/ld+json")
        .header("link", context_link_header())
        .send()
        .await
        .expect("csource subscription query");
    assert_status(&response, 200);
    assert!(json_body(&response).is_array(), "a csource subscription query returns an array");
    let _cleanup = delete_sub(&topology, &sub_id).await;
    topology.finish().await.expect("topology cleanup");
}

/// Csource subscription retrieve returns `200`/`400`/`404` (clause 6.11.3.1).
/// Confirms REQ-HTTP-89.
#[tokio::test]
async fn dist_http_retrieve_csource_subscription_returns_200_400_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let sub_id = topology.id("Subscription:cssub-retrieve");
    assert_status(&create_sub(&topology, &subscription_body(&sub_id)).await, 201);

    assert_status(&get_sub(&topology, &sub_id).await, 200);
    assert_status(&get_sub(&topology, "not a uri").await, 400);
    assert_status(&get_sub(&topology, &topology.id("Subscription:cssub-retrieve-unknown")).await, 404);
    let _cleanup = delete_sub(&topology, &sub_id).await;
    topology.finish().await.expect("topology cleanup");
}

/// Csource subscription update returns `204`/`400`/`404` (clause 6.11.3.3).
/// Confirms REQ-HTTP-90.
#[tokio::test]
async fn dist_http_update_csource_subscription_returns_204_400_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let sub_id = topology.id("Subscription:cssub-update");
    assert_status(&create_sub(&topology, &subscription_body(&sub_id)).await, 201);

    assert_status(&patch_sub(&topology, &sub_id, &json!({ "description": "updated" })).await, 204);
    assert_status(&patch_sub(&topology, &sub_id, &json!({ "expiresAt": "not-a-datetime" })).await, 400);
    assert_status(
        &patch_sub(&topology, &topology.id("Subscription:cssub-update-unknown"), &json!({ "description": "x" })).await,
        404,
    );
    let _cleanup = delete_sub(&topology, &sub_id).await;
    topology.finish().await.expect("topology cleanup");
}

/// Csource subscription delete returns `204`/`400`/`404` (clause 6.11.3.4).
/// Confirms REQ-HTTP-91.
#[tokio::test]
async fn dist_http_delete_csource_subscription_returns_204_400_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let sub_id = topology.id("Subscription:cssub-delete");
    assert_status(&create_sub(&topology, &subscription_body(&sub_id)).await, 201);

    assert_status(&delete_sub(&topology, "not a uri").await, 400);
    assert_status(&delete_sub(&topology, &topology.id("Subscription:cssub-delete-unknown")).await, 404);
    assert_status(&delete_sub(&topology, &sub_id).await, 204);
    topology.finish().await.expect("topology cleanup");
}

/// The source-identity resource returns `200` with a `ContextSourceIdentity`
/// (Andromeda's genuine branch) or `501` where unsupported (clause 6.33.3.1).
/// Confirms REQ-HTTP-96.
#[tokio::test]
async fn dist_http_source_identity_returns_200_or_501() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .source_identity(Some("application/ld+json"))
        .await
        .expect("source identity");
    assert!(
        [200, 501].contains(&response.status.as_u16()),
        "source identity returns 200 or 501, got {}",
        response.status.as_u16()
    );
    if response.status.as_u16() == 200 {
        assert!(
            json_body(&response)["contextSourceAlias"].is_string(),
            "the 200 branch returns a ContextSourceIdentity with an alias"
        );
    }
    topology.finish().await.expect("topology cleanup");
}
