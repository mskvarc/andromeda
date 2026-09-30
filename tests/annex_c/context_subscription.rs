//! Clause C.4: the Context Subscription example.
//!
//! The subscription watches `speed` on Vehicles and notifies an endpoint that
//! does not exist, so it is deleted before anything is asserted.
//!
//! The printed `geoQ` names the reference position `[-1,100]`: a latitude of
//! 100, off the WGS 84 axes RFC 7946 section 4 places every position on.
//! Clause 4.10 requires the reference coordinates "as mandated by IETF RFC 7946
//! [8], section 3.1.1", and Annex C is informative, so the broker refuses the
//! position as printed. The example is exercised with its two ordinates
//! exchanged, the in-range position `[100,-1]`, and the printed one is pinned
//! as refused.

use crate::{
    BASE_URL,
    clean_tenant,
    comparison::{ContextMember, EntityOrder, assert_matches_example, corpus},
    context_link,
    example_context::hosted_contexts,
    get_json,
};
use andromeda_test_fixtures::problem_detail::BAD_REQUEST_DATA;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The subscription exactly as clause C.4 prints it.
const SUBSCRIPTION: &str = include_str!("corpus/c-4-subscription.json");
/// The identifier the printed subscription claims.
const SUBSCRIPTION_ID: &str = "urn:ngsi-ld:Subscription:mySubscription";

/// The printed subscription with its `geoQ` reference position moved onto the
/// WGS 84 axes by exchanging its two ordinates, everything else as printed.
fn with_in_range_reference(mut subscription: Value) -> Value {
    subscription["geoQ"]["coordinates"] = json!([100, -1]);
    subscription
}

#[tokio::test]
async fn annex_c_4_context_subscription_example() {
    let environment = clean_tenant().await;
    let link = context_link(hosted_contexts().url("aggregatedContext.jsonld").as_str());
    let expected = with_in_range_reference(corpus(SUBSCRIPTION));
    let http = environment.client();

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

    environment.finish().await;
}

/// Clause 4.10 with RFC 7946 section 4: the reference position C.4 prints,
/// latitude 100, is no WGS 84 position, so the subscription is refused as
/// `BadRequestData` naming the latitude, and nothing is created.
#[tokio::test]
async fn annex_c_4_printed_reference_position_is_refused_as_off_the_wgs84_axes() {
    let environment = clean_tenant().await;
    let link = context_link(hosted_contexts().url("aggregatedContext.jsonld").as_str());
    let http = environment.client();

    let response = http
        .post(format!("{BASE_URL}/subscriptions/"))
        .header("Content-Type", "application/json")
        .header("Link", &link)
        .json(&crate::without_context(&corpus(SUBSCRIPTION)))
        .send()
        .await
        .expect("subscription creation request");
    let status = response.status();
    let problem: Value = response.json().await.unwrap_or_default();
    let (retrieved, _body) = get_json(&http, &format!("/subscriptions/{SUBSCRIPTION_ID}"), "application/ld+json", Some(&link)).await;
    let _deleted = http.delete(format!("{BASE_URL}/subscriptions/{SUBSCRIPTION_ID}")).send().await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "the printed C.4 reference position must be refused: {problem}");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA.as_str()), "{problem}");
    let detail = problem["detail"].as_str().unwrap_or_default();
    assert!(detail.contains("latitude 100 is outside [-90, 90]"), "the detail names the latitude: {detail}");
    assert_eq!(retrieved, StatusCode::NOT_FOUND, "a refused subscription is not created");

    environment.finish().await;
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
