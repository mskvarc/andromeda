//! The clause 4.21 selectors a request may not combine.
//!
//! Three members shape one projection. ETSI GS CIM 009 v1.9.1 Table 6.4.3.2-1
//! describes `pick` as reducing every Entity "down to only contain the
//! specified Entity members" and `omit` as removing them, so the two select
//! opposite ends of one projection; `attrs` is by definition redundant with
//! `pick`, which Tables 6.4.3.2-1 and 5.2.14.1-1 say in the same words: "A
//! synonym for pick, except that id, type, scope are not allowed. Deprecated".
//!
//! The rule holds on the query bindings, which state it four separate times, and
//! on Subscriptions, where Table 5.2.14.1-1 gives `NotificationParams` the same
//! three members, `attributes` being the deprecated synonym's name there. A
//! subscription naming `pick` and `omit`, or either beside `attributes`, is
//! refused rather than created with `201` and notified under whichever of the
//! two the renderer happens to read. Clause 5.11.2.3 takes
//! the clause 5.2.12 data type unchanged for a Context Source Registration
//! Subscription, so it carries the rule too.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{header::jsonld_link_header, response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::{CORE_CONTEXT, TEST_SUITE_CONTEXT};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The problem type clause 5.5.4 gives content the operation cannot accept.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// The temporal window a temporal query needs beside the selectors (clause 4.11).
const WINDOW: &str = "timerel=after&timeAt=2000-01-01T00:00:00Z";

/// The three combinations the rule refuses, as URL parameters.
const REFUSED_PARAMETERS: [&str; 3] = ["pick=speed&omit=speed", "pick=speed&attrs=speed", "omit=speed&attrs=speed"];

/// The three combinations the rule admits, one selector each.
const ADMITTED_PARAMETERS: [&str; 3] = ["pick=speed", "omit=speed", "attrs=speed"];

/// One Vehicle, so the reads address something real.
fn vehicle(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 60, "observedAt": "2025-01-01T00:00:00Z"},
        "@context": [CORE_CONTEXT]
    })
}

/// A Subscription whose `notification` carries exactly the named selectors.
fn subscription_with(id: &str, selectors: &[(&str, Value)]) -> Value {
    let mut notification = json!({"endpoint": {"uri": "http://localhost:1/notify", "accept": "application/json"}});
    for (member, value) in selectors {
        notification[*member] = value.clone(); // clone: the fixture owns the member the table names
    }
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": notification,
        "@context": [CORE_CONTEXT]
    })
}

/// The selector combinations Table 5.2.14.1-1 refuses inside a `notification`.
fn refused_notification_selectors() -> [Vec<(&'static str, Value)>; 3] {
    [
        vec![("pick", json!(["id"])), ("omit", json!(["id"]))],
        vec![("pick", json!(["id"])), ("attributes", json!(["id"]))],
        vec![("omit", json!(["id"])), ("attributes", json!(["id"]))],
    ]
}

/// Create the entity the reads address.
async fn create_entity(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// Send one read against a root-relative path.
async fn read(client: &TestClient, path: &str) -> TestResponse {
    client
        .raw(Method::GET, path)
        .header("Accept", "application/ld+json")
        .send()
        .await
        .expect("read request")
}

/// Assert a refusal is the `BadRequestData` of clause 5.5.4.
fn assert_refused(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{scenario}: {problem}");
    assert!(problem["title"].is_string(), "{scenario}: a problem carries a title: {problem}");
}

/// The rule holds on every binding that carries the three members on its URL.
#[tokio::test]
async fn regression_projection_selectors_a_url_cannot_combine_two_of_them() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:projection-selectors");
    create_entity(client, &vehicle(&id)).await;

    for parameters in REFUSED_PARAMETERS {
        assert_refused(
            &read(client, &format!("entities/{id}?{parameters}")).await,
            &format!("retrieve with {parameters}"),
        );
        assert_refused(
            &read(client, &format!("entities?type=Vehicle&{parameters}")).await,
            &format!("query with {parameters}"),
        );
        assert_refused(
            &read(client, &format!("temporal/entities/{id}?{WINDOW}&{parameters}")).await,
            &format!("temporal retrieve with {parameters}"),
        );
        assert_refused(
            &read(client, &format!("temporal/entities?type=Vehicle&{WINDOW}&{parameters}")).await,
            &format!("temporal query with {parameters}"),
        );
    }

    for parameters in ADMITTED_PARAMETERS {
        let retrieved = read(client, &format!("entities/{id}?{parameters}")).await;
        assert_eq!(retrieved.status, StatusCode::OK, "retrieve with {parameters}: {:?}", retrieved.body);
    }

    scope.finish().await.expect("cleanup failed");
}

/// The clause 5.2.23 POST body carries the same three members, so it cannot
/// smuggle a combination past the rule the GET binding enforces.
#[tokio::test]
async fn regression_projection_selectors_a_posted_query_body_cannot_combine_two_of_them() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for selectors in [
        json!({"pick": ["speed"], "omit": ["speed"]}),
        json!({"pick": ["speed"], "attrs": ["speed"]}),
        json!({"omit": ["speed"], "attrs": ["speed"]}),
    ] {
        let mut body = json!({"type": "Query", "entities": [{"type": "Vehicle"}]});
        for (member, value) in selectors.as_object().expect("the fixture is an object") {
            body[member] = value.clone(); // clone: the request body owns the members under test
        }
        let posted = client
            .raw(Method::POST, "entityOperations/query")
            .header("Content-Type", "application/json")
            .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"))
            .json(&body)
            .send()
            .await
            .expect("posted query");
        assert_refused(&posted, &format!("posted query with {selectors}"));
    }

    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.14.1-1 gives `NotificationParams` the same three members under the
/// names `pick`, `omit` and `attributes`, so a Subscription carries the rule.
#[tokio::test]
async fn regression_projection_selectors_a_subscription_notification_cannot_combine_two_of_them() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (index, selectors) in refused_notification_selectors().iter().enumerate() {
        let id = scope.id(&format!("Subscription:projection-refused-{index}"));
        let created = client
            .create_subscription(&subscription_with(&id, selectors))
            .await
            .expect("create subscription");
        assert_refused(&created, &format!("subscription with {selectors:?}"));
    }

    // One selector alone shapes one projection and is admitted.
    for (index, member) in ["pick", "omit", "attributes"].into_iter().enumerate() {
        let id = scope.id(&format!("Subscription:projection-admitted-{index}"));
        let created = client
            .create_subscription(&subscription_with(&id, &[(member, json!(["id"]))]))
            .await
            .expect("create subscription");
        assert_eq!(created.status, StatusCode::CREATED, "subscription with {member} alone: {:?}", created.body);
        client.delete_subscription(&id).await.expect("delete subscription");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.8.2 updates a Subscription with a fragment, and a fragment that
/// replaces the `notification` must satisfy the same rule the creation did.
#[tokio::test]
async fn regression_projection_selectors_a_subscription_update_cannot_introduce_a_combination() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Subscription:projection-update");

    let created = client
        .create_subscription(&subscription_with(&id, &[("pick", json!(["id"]))]))
        .await
        .expect("create subscription");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    for selectors in refused_notification_selectors() {
        let mut notification = json!({"endpoint": {"uri": "http://localhost:1/notify", "accept": "application/json"}});
        for (member, value) in &selectors {
            notification[*member] = value.clone(); // clone: the fragment owns the members under test
        }
        let updated = client
            .update_subscription(&id, &json!({"notification": notification}), Some(CORE_CONTEXT))
            .await
            .expect("update subscription");
        assert_refused(&updated, &format!("update with {selectors:?}"));
    }

    client.delete_subscription(&id).await.expect("delete subscription");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.11.2.3 takes the clause 5.2.12 data type unchanged, so a Context
/// Source Registration Subscription carries the rule as written.
#[tokio::test]
async fn regression_projection_selectors_a_context_source_subscription_carries_the_rule() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (index, selectors) in refused_notification_selectors().iter().enumerate() {
        let id = scope.id(&format!("Subscription:csource-projection-{index}"));
        let created = client
            .raw(Method::POST, "csourceSubscriptions")
            .header("content-type", "application/ld+json")
            .json(&subscription_with(&id, selectors))
            .send()
            .await
            .expect("create context source subscription");
        assert_refused(&created, &format!("context source subscription with {selectors:?}"));
    }

    scope.finish().await.expect("cleanup failed");
}
