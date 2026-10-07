//! Content-derived identifiers of `ImplicitlyCreated` entries.
//!
//! The identifier is the version 5 UUID of the stored document's RFC 8785
//! canonical form, so member order, whitespace and number spelling do not
//! change it, while the order of an `@context` array, which JSON-LD processes
//! in order, does.

use crate::{
    broker_endpoints::broker_url,
    context_api::{extract_id, implicit_context_ids},
    identity::uuid_version,
    ngsi_requests::{delete_subscription, jsonld_context_of},
    private_tenant::PrivateTenant,
};
use reqwest::{Client, StatusCode};
use uuid::Version;

/// Create a subscription from the raw `application/ld+json` text `body`, so
/// the test controls member order and whitespace exactly. Returns its id.
async fn subscribe_raw(client: &Client, body: String) -> String {
    let response = client
        .post(broker_url("subscriptions"))
        .header("Content-Type", "application/ld+json")
        .body(body)
        .send()
        .await
        .expect("create subscription request failed");
    assert_eq!(response.status(), StatusCode::CREATED, "{:?}", response.text().await);
    extract_id(
        response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok())
            .expect("a Location header"),
    )
}

/// A subscription body naming `context` (raw JSON text), notified as
/// `application/json`, so the broker hosts the context to name it by one URL.
fn subscription_text(context: &str) -> String {
    format!(
        r#"{{"type": "Subscription", "entities": [{{"type": "IdentityProbe"}}], "notification": {{"endpoint": {{"uri": "http://127.0.0.1:1/notify", "accept": "application/json"}}}}, "@context": {context}}}"#
    )
}

/// Two subscriptions with the same `@context` array notify under
/// the same hosted URL, and the tenant holds that array once, under a version
/// 5 identifier.
#[tokio::test]
async fn subscriptions_with_the_same_context_array_share_one_implicit_context() {
    let tenant = PrivateTenant::provisioned().await;
    let context = r#"[{"speed": "https://example.org/speed"}, {"IdentityProbe": "https://example.org/IdentityProbe"}]"#;

    let first = subscribe_raw(tenant.client(), subscription_text(context)).await;
    let second = subscribe_raw(tenant.client(), subscription_text(context)).await;

    let first_context = jsonld_context_of(tenant.client(), &first).await;
    assert_eq!(first_context, jsonld_context_of(tenant.client(), &second).await, "both notify under one URL");
    let implicit = implicit_context_ids(tenant.client()).await;
    assert_eq!(implicit.len(), 1, "one hosted document for one array: {implicit:?}");
    assert_eq!(uuid_version(&implicit[0]), Some(Version::Sha1));
    assert_eq!(extract_id(&first_context), implicit[0], "{first_context} names {}", implicit[0]);
    delete_subscription(tenant.client(), &first).await;
    delete_subscription(tenant.client(), &second).await;
    tenant.finish().await;
}

/// The same inline definitions written with members in another
/// order and other whitespace derive the same identifier. (Number spelling is
/// proven on the canonical form itself, in `andromeda-context-registry`'s
/// `canonical_document` unit tests: no NGSI-LD `@context` member takes a number
/// a client may spell two ways.)
#[tokio::test]
async fn formatting_and_member_order_do_not_split_an_implicit_context() {
    let tenant = PrivateTenant::provisioned().await;
    let tidy = r#"[{"speed": "https://example.org/speed", "IdentityProbe": "https://example.org/IdentityProbe"}, {"rate": {"@id": "https://example.org/rate", "@type": "@id"}}]"#;
    let untidy = "[ {\n  \"IdentityProbe\" :\"https://example.org/IdentityProbe\",\n\t\"speed\":  \"https://example.org/speed\" } ,{\"rate\":{ \"@type\":\"@id\" , \"@id\" : \"https://example.org/rate\"}}]";

    let first = subscribe_raw(tenant.client(), subscription_text(tidy)).await;
    let second = subscribe_raw(tenant.client(), subscription_text(untidy)).await;

    assert_eq!(
        jsonld_context_of(tenant.client(), &first).await,
        jsonld_context_of(tenant.client(), &second).await
    );
    assert_eq!(implicit_context_ids(tenant.client()).await.len(), 1);
    delete_subscription(tenant.client(), &first).await;
    delete_subscription(tenant.client(), &second).await;
    tenant.finish().await;
}

/// Different arrays are different documents, including the same
/// contexts in another order.
#[tokio::test]
async fn subscriptions_with_different_context_arrays_get_distinct_implicit_contexts() {
    let tenant = PrivateTenant::provisioned().await;
    let speed = r#"{"speed": "https://example.org/speed"}"#;
    let probe = r#"{"IdentityProbe": "https://example.org/IdentityProbe"}"#;

    let forward = subscribe_raw(tenant.client(), subscription_text(&format!("[{speed}, {probe}]"))).await;
    let backward = subscribe_raw(tenant.client(), subscription_text(&format!("[{probe}, {speed}]"))).await;

    assert_ne!(
        jsonld_context_of(tenant.client(), &forward).await,
        jsonld_context_of(tenant.client(), &backward).await
    );
    let implicit = implicit_context_ids(tenant.client()).await;
    assert_eq!(implicit.len(), 2, "{implicit:?}");
    assert!(
        implicit.iter().all(|identifier| uuid_version(identifier) == Some(Version::Sha1)),
        "{implicit:?}"
    );
    delete_subscription(tenant.client(), &forward).await;
    delete_subscription(tenant.client(), &backward).await;
    tenant.finish().await;
}
