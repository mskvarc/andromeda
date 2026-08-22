//! `NGSILD-Tenant` scoping and the `count` / `NGSILD-Results-Count` / `limit=0`
//! interaction (clause 6.3.4, 6.3.10, 6.3.13): a non-default tenant is echoed on
//! responses and notifications and omitted for the default tenant, while
//! `count=true` reports the unpaginated total and `limit=0` requires `count`.

use crate::{
    entity::vehicle,
    http::forwarding::context_link,
    notification_adapter::NotificationAdapter,
    oracle::{assert_status, json_body, results_count_header},
    topology::{self, BrokerRole},
};
use andromeda_test_client::{response::TestResponse, tenant_name::TenantName};
use serde_json::json;
use std::time::Duration;

/// The `NGSILD-Tenant` header value on a response, if present.
fn tenant_header(response: &TestResponse) -> Option<String> {
    response.headers.get("ngsild-tenant").and_then(|value| value.to_str().ok()).map(str::to_owned)
}

/// A regex that isolates this run's entities by their shared id prefix, so a
/// `type=Vehicle` query counts only what the test created.
fn prefix_pattern(topology: &andromeda_test_harness::multi_broker::MultiBrokerTopology<BrokerRole>) -> String {
    urlencoding::encode(topology.id_prefix()).into_owned()
}

/// A response under a non-default tenant carries the `NGSILD-Tenant` header
/// (clause 6.3.4). Confirms REQ-HTTP-12.
#[tokio::test]
async fn dist_http_non_default_tenant_response_carries_tenant_header() {
    let topology = topology::single().await.expect("single-broker topology");
    let tenant = TenantName::parse("distopTenantA").expect("valid tenant name");
    let client = topology.broker(BrokerRole::A).tenant_client(tenant);
    let entity_id = topology.id("Vehicle:tenant-header");

    assert_status(
        &client.post_entity(&vehicle(&entity_id, 5)).with_ld_json().send().await.expect("tenant create"),
        201,
    );
    let response = client
        .get_entity(&entity_id, "", "application/ld+json", Some(context_link()))
        .await
        .expect("tenant retrieve");
    assert_status(&response, 200);
    assert_eq!(
        tenant_header(&response).as_deref(),
        Some("distopTenantA"),
        "a non-default tenant response echoes NGSILD-Tenant"
    );

    assert_status(&client.delete_entity(&entity_id).await.expect("tenant delete"), 204);
    topology.finish().await.expect("topology cleanup");
}

/// A response under the default tenant omits the `NGSILD-Tenant` header
/// (clause 6.3.4). Confirms REQ-HTTP-13.
#[tokio::test]
async fn dist_http_default_tenant_response_omits_tenant_header() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:default-tenant");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("default create"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .get_entity(&entity_id, "", "application/ld+json", Some(context_link()))
        .await
        .expect("default retrieve");
    assert_status(&response, 200);
    assert_eq!(tenant_header(&response), None, "a default-tenant response omits NGSILD-Tenant");

    topology.finish().await.expect("topology cleanup");
}

/// The tenant carried on a request is echoed unchanged on its response
/// (clause 6.3.4). Confirms REQ-HTTP-14.
#[tokio::test]
async fn dist_http_tenant_header_echoed_in_response() {
    let topology = topology::single().await.expect("single-broker topology");
    let tenant = TenantName::parse("distopTenantEcho").expect("valid tenant name");
    let client = topology.broker(BrokerRole::A).tenant_client(tenant);
    let entity_id = topology.id("Vehicle:tenant-echo");

    let created = client.post_entity(&vehicle(&entity_id, 5)).with_ld_json().send().await.expect("tenant create");
    assert_status(&created, 201);
    assert_eq!(
        tenant_header(&created).as_deref(),
        Some("distopTenantEcho"),
        "the create response echoes the request tenant"
    );

    assert_status(&client.delete_entity(&entity_id).await.expect("tenant delete"), 204);
    topology.finish().await.expect("topology cleanup");
}

/// A notification dispatched for a tenant-scoped subscription carries the
/// `NGSILD-Tenant` header (clause 6.3.4). Confirms REQ-HTTP-15.
#[tokio::test]
async fn dist_http_tenant_scoped_notification_carries_tenant_header() {
    let topology = topology::single().await.expect("single-broker topology");
    let tenant = TenantName::parse("distopTenantNotify").expect("valid tenant name");
    let client = topology.broker(BrokerRole::A).tenant_client(tenant);
    let receiver = NotificationAdapter::start().await;
    let sub_id = topology.id("Subscription:tenant-notify");
    let entity_id = topology.id("Vehicle:tenant-notify");

    let subscription = json!({
        "id": sub_id,
        "type": "Subscription",
        "entities": [ { "type": "Vehicle" } ],
        "notification": { "endpoint": { "uri": receiver.endpoint_url(), "accept": "application/json" } },
        "@context": context_link()
    });
    assert_status(&client.create_subscription(&subscription).await.expect("tenant subscription create"), 201);
    assert_status(
        &client
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("tenant trigger create"),
        201,
    );

    let notification = receiver.wait_for_subscription_notification(&sub_id, Duration::from_secs(10)).await;
    let carried = notification.headers.get("ngsild-tenant").and_then(|value| value.to_str().ok());
    assert_eq!(carried, Some("distopTenantNotify"), "a tenant-scoped notification carries NGSILD-Tenant");

    assert_status(&client.delete_subscription(&sub_id).await.expect("tenant subscription delete"), 204);
    assert_status(&client.delete_entity(&entity_id).await.expect("tenant entity delete"), 204);
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// `count=true` is honoured on a query, adding the `NGSILD-Results-Count` header
/// (clause 6.3.13). Confirms REQ-HTTP-8.
#[tokio::test]
async fn dist_http_count_param_honoured_on_query() {
    let topology = topology::single().await.expect("single-broker topology");
    for index in 0..3 {
        let id = topology.id(&format!("Vehicle:count-{index}"));
        assert_status(
            &topology
                .broker(BrokerRole::A)
                .client()
                .post_entity(&vehicle(&id, 5))
                .with_ld_json()
                .send()
                .await
                .expect("create"),
            201,
        );
    }

    let query = format!("type=Vehicle&idPattern={}&count=true", prefix_pattern(&topology));
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .query_entities(&query, "application/ld+json", Some(context_link()))
        .await
        .expect("counted query");
    assert_status(&response, 200);
    assert_eq!(results_count_header(&response), Some(3), "count=true reports the number of matches");

    topology.finish().await.expect("topology cleanup");
}

/// `count=true` reports the unpaginated total even under a `limit` (clause
/// 6.3.13). Confirms REQ-HTTP-9.
#[tokio::test]
async fn dist_http_count_true_returns_results_count_header_total() {
    let topology = topology::single().await.expect("single-broker topology");
    for index in 0..3 {
        let id = topology.id(&format!("Vehicle:total-{index}"));
        assert_status(
            &topology
                .broker(BrokerRole::A)
                .client()
                .post_entity(&vehicle(&id, 5))
                .with_ld_json()
                .send()
                .await
                .expect("create"),
            201,
        );
    }

    let query = format!("type=Vehicle&idPattern={}&count=true&limit=1", prefix_pattern(&topology));
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .query_entities(&query, "application/ld+json", Some(context_link()))
        .await
        .expect("counted paged query");
    assert_status(&response, 200);
    assert_eq!(results_count_header(&response), Some(3), "the count is the total, independent of limit");
    assert_eq!(json_body(&response).as_array().map(Vec::len), Some(1), "the page still honours limit=1");

    topology.finish().await.expect("topology cleanup");
}

/// `limit=0` with `count=true` returns an empty array and the total count
/// (clause 6.3.13). Confirms REQ-HTTP-10.
#[tokio::test]
async fn dist_http_limit_zero_with_count_returns_empty_array_and_count() {
    let topology = topology::single().await.expect("single-broker topology");
    for index in 0..3 {
        let id = topology.id(&format!("Vehicle:zero-{index}"));
        assert_status(
            &topology
                .broker(BrokerRole::A)
                .client()
                .post_entity(&vehicle(&id, 5))
                .with_ld_json()
                .send()
                .await
                .expect("create"),
            201,
        );
    }

    let query = format!("type=Vehicle&idPattern={}&count=true&limit=0", prefix_pattern(&topology));
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .query_entities(&query, "application/ld+json", Some(context_link()))
        .await
        .expect("limit-zero query");
    assert_status(&response, 200);
    assert_eq!(json_body(&response).as_array().map(Vec::len), Some(0), "limit=0 returns an empty page");
    assert_eq!(results_count_header(&response), Some(3), "limit=0 still reports the total count");

    topology.finish().await.expect("topology cleanup");
}

/// `limit=0` without `count` is rejected with `400` (clause 6.3.13).
/// Confirms REQ-HTTP-11.
#[tokio::test]
async fn dist_http_limit_zero_without_count_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .query_entities("type=Vehicle&limit=0", "application/ld+json", Some(context_link()))
        .await
        .expect("limit-zero query");
    assert_status(&response, 400);
    topology.finish().await.expect("topology cleanup");
}
