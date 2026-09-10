//! Clause C.4: the Context Subscription example.
//!
//! The subscription watches `speed` on Vehicles and notifies an endpoint that
//! does not exist, so it is deleted before anything is asserted.

use crate::{
    BASE_URL,
    clean_tenant,
    client,
    comparison::{ContextMember, EntityOrder, assert_matches_example, corpus},
    context_link,
    example_context::hosted_contexts,
    get_json,
};
use reqwest::StatusCode;
use serde_json::Value;
use serial_test::serial;

/// The subscription exactly as clause C.4 prints it.
const SUBSCRIPTION: &str = include_str!("corpus/c-4-subscription.json");
/// The identifier the printed subscription claims.
const SUBSCRIPTION_ID: &str = "urn:ngsi-ld:Subscription:mySubscription";

#[tokio::test]
#[serial]
async fn annex_c_4_context_subscription_example() {
    let environment = clean_tenant().await;
    let link = context_link(hosted_contexts().url("aggregatedContext.jsonld").as_str());
    let expected = corpus(SUBSCRIPTION);
    let http = client();

    let response = http
        .post(format!("{BASE_URL}/subscriptions/"))
        .header("Content-Type", "application/json")
        .header("Link", &link)
        .json(&crate::without_context(&expected))
        .send()
        .await
        .expect("subscription creation request");
    let created = response.status();
    let refusal = response.text().await.unwrap_or_default();

    let retrieved = if created == StatusCode::CREATED {
        let (status, body) = get_json(&http, &format!("/subscriptions/{SUBSCRIPTION_ID}"), "application/ld+json", Some(&link)).await;
        Some((status, body))
    } else {
        None
    };

    let _deleted = http.delete(format!("{BASE_URL}/subscriptions/{SUBSCRIPTION_ID}")).send().await;

    assert_eq!(created, StatusCode::CREATED, "the C.4 subscription example was refused: {refusal}");
    let (status, actual) = retrieved.expect("a created subscription is retrievable");
    assert_eq!(status, StatusCode::OK, "subscription retrieval failed: {actual}");
    assert_matches_example(
        "C.4",
        &expected,
        &without_operational_members(actual),
        ContextMember::Ignored,
        EntityOrder::AsPrinted,
    );

    environment.reset_and_finish().await.expect("tenant reset");
}

/// Drop the members clause 5.2.12 marks output-only or defaulted.
///
/// `status`, `notification.status` and the delivery counters report how the
/// subscription has been getting on; `isActive`, `origin` and
/// `notificationTrigger` are the defaults clause 5.2.12 defines and the broker
/// fills in. None of them is part of the subscription the example prints.
fn without_operational_members(mut subscription: Value) -> Value {
    if let Some(members) = subscription.as_object_mut() {
        for member in [
            "status",
            "isActive",
            "origin",
            "jsonldContext",
            "timeInterval",
            "throttling",
            "notificationTrigger",
        ] {
            members.remove(member);
        }
        if let Some(notification) = members.get_mut("notification").and_then(Value::as_object_mut) {
            for member in [
                "status",
                "timesSent",
                "lastNotification",
                "lastSuccess",
                "lastFailure",
                "showChanges",
                "sysAttrs",
            ] {
                notification.remove(member);
            }
        }
    }
    subscription
}
