//! When a Context Source Registration Subscription is notified, and what the
//! notification carries.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.11.2.4 states the cadence in two bullets
//! that differ in more than timing. A subscription defining a `timeInterval`
//! is owed a `CsourceNotification` "periodically, initially on subscription
//! and when the time interval (in seconds) specified in such value field is
//! reached, independent of any changes to the set of Context Source
//! registrations"; the schedule alone decides, so the notification goes out
//! carrying whatever matches at that instant, the empty set included. One
//! without a `timeInterval` is owed a notification "initially on subscription
//! and whenever there is a change of a matching Context Source Registration
//! (creation, update, deletion) ... informing about this change", so while
//! nothing matches there is no change to inform about and nothing is sent.
//! The broker read the first bullet as belonging to the periodic worker alone
//! and never notified an interval subscription on creation, and the periodic
//! worker in turn skipped an empty match set and omitted the `triggerReason`
//! Table 5.3.2-1 gives cardinality 1.
//!
//! Clause 5.11.7 also has the notification carry "filtered Context Source
//! Registrations, which only contain context source registration information
//! relevant for the subscription". That is a notification body, so its entity
//! types are compacted back through the @context the notification is sent
//! under; they were being written out in the expanded form the store holds.
//!
//! Clauses 5.10.2.4 and 5.11.2.4 both match a temporal query against a
//! registration's `observationInterval` or `managementInterval`, and word the
//! `before` and `after` relations as "the timeAt ... is contained in or is an
//! endpoint of a time period included in the specified time interval". Read as
//! containment that answers the wrong question: a source covering August 2020
//! to August 2021 plainly holds observations made after June 2020. Every
//! relation is now the overlap the same clause states outright for `between`,
//! and the in-memory subscription match and the SQL discovery query implement
//! it identically -- a client that reaches a source through one must reach it
//! through the other.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::notification_adapter::NotificationAdapter;
use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;
use urlencoding::encode;

/// Long enough that no second tick can fire inside a test, so a notification
/// that does arrive is the one creation owed.
const LONG_INTERVAL_SECONDS: u64 = 600;

/// How long a notification that is owed is waited for.
const ARRIVAL: Duration = Duration::from_secs(10);

/// How long a notification that is not owed is waited against.
const SILENCE: Duration = Duration::from_secs(5);

/// The `observationInterval` and `managementInterval` the temporal cases
/// register: a source that answers for the year from August 2020.
fn coverage_interval() -> Value {
    json!({"startAt": "2020-08-01T22:07:00Z", "endAt": "2021-08-01T21:07:00Z"})
}

/// A Context Source Registration Subscription watching `entity_type`.
fn csource_subscription(id: &str, entity_type: &str, endpoint: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": entity_type}],
        "notification": {"format": "keyValues", "endpoint": {"uri": endpoint, "accept": "application/json"}},
        "@context": [CORE_CONTEXT]
    })
}

/// A registration covering `entity_type`, optionally over a time interval.
fn registration(id: &str, entity_type: &str, interval: Option<(&str, Value)>) -> Value {
    let mut body = json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": entity_type}]}],
        "endpoint": "http://localhost:9999/source",
        "@context": [CORE_CONTEXT]
    });
    if let Some((member, value)) = interval {
        body[member] = value;
    }
    body
}

/// Create one Context Source Registration Subscription.
async fn create_csource_subscription(client: &TestClient, body: &Value) -> TestResponse {
    client
        .raw(Method::POST, "csourceSubscriptions")
        .header("content-type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create context source subscription")
}

/// Create one Context Source Registration Subscription and assert it was taken.
async fn subscribe(client: &TestClient, body: &Value) {
    let created = create_csource_subscription(client, body).await;
    assert_eq!(created.status, StatusCode::CREATED, "create subscription: {:?}", created.body);
}

/// Register one Context Source and assert it was taken.
async fn register(client: &TestClient, body: &Value) {
    let created = client.create_registration(body).await.expect("create registration");
    assert_eq!(created.status, StatusCode::CREATED, "create registration: {:?}", created.body);
}

/// Delete a Context Source Registration Subscription, so the tenant's interval
/// sweep does not keep notifying after the test that owns it has finished.
async fn unsubscribe(client: &TestClient, id: &str) {
    client
        .raw(Method::DELETE, &format!("csourceSubscriptions/{}", encode(id)))
        .send()
        .await
        .expect("delete context source subscription");
}

/// The entry a notification carries for `registration_id`.
///
/// Membership rather than equality: the match set is the tenant's, so a
/// registration another case in the suite registered legitimately travels with
/// it. What each case owns is whether *its* registration is reported.
fn notified_entry<'a>(body: &'a Value, registration_id: &str, reason: &str) -> &'a Value {
    let entry = body["data"]
        .as_array()
        .expect("a CsourceNotification carries a data array")
        .iter()
        .find(|registration| registration["id"] == json!(registration_id));
    assert!(entry.is_some(), "{reason}: {body}");
    entry.expect("the assertion above refuses a notification that does not report it")
}

/// Assert that a notification reports `registration_id`.
fn assert_notified(body: &Value, registration_id: &str, reason: &str) {
    let _entry = notified_entry(body, registration_id, reason);
}

/// Withdraw a registration, so the tenant's match set does not carry it into
/// the next case.
async fn deregister(client: &TestClient, id: &str) {
    client.delete_registration(id).await.expect("delete registration");
}

/// Clause 5.11.2.4 owes a `timeInterval` subscription a notification
/// "initially on subscription" as well as on every elapsed interval, so a
/// subscription created over a matching registration is notified at once
/// rather than after its first interval.
#[tokio::test]
async fn regression_csource_notification_a_periodic_subscription_is_notified_on_creation() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let registration_id = scope.id("ContextSourceRegistration:periodic-initial");
    let subscription_id = scope.id("Subscription:periodic-initial");

    register(client, &registration(&registration_id, "Building", None)).await;
    let mut subscription = csource_subscription(&subscription_id, "Building", &receiver.endpoint_url());
    subscription["timeInterval"] = json!(LONG_INTERVAL_SECONDS);
    subscribe(client, &subscription).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    assert_notified(
        &notification.body,
        &registration_id,
        "the initial notification reports the registrations that match now",
    );

    unsubscribe(client, &subscription_id).await;
    deregister(client, &registration_id).await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.11.2.4 sends a `timeInterval` subscription's notification
/// "independent of any changes to the set of Context Source registrations", so
/// a schedule that comes due over an empty match set still reports -- with an
/// empty `data`, which is the current answer to what was asked.
#[tokio::test]
async fn regression_csource_notification_a_periodic_subscription_reports_an_empty_match_set() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let subscription_id = scope.id("Subscription:periodic-empty");

    let mut subscription = csource_subscription(&subscription_id, "NothingIsRegisteredUnderThis", &receiver.endpoint_url());
    subscription["timeInterval"] = json!(LONG_INTERVAL_SECONDS);
    subscribe(client, &subscription).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    assert_eq!(
        notification.body["data"],
        json!([]),
        "nothing matches, so the notification reports nothing: {}",
        notification.body
    );

    unsubscribe(client, &subscription_id).await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.11.2.4 ties a subscription without a `timeInterval` to "a change of
/// a matching Context Source Registration", so one created while nothing
/// matches has no change to report and stays silent.
#[tokio::test]
async fn regression_csource_notification_a_change_driven_subscription_is_silent_without_a_match() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let subscription_id = scope.id("Subscription:change-driven-empty");

    subscribe(
        client,
        &csource_subscription(&subscription_id, "NothingIsRegisteredUnderThisEither", &receiver.endpoint_url()),
    )
    .await;

    receiver.wait_for_no_subscription_notification(&subscription_id, SILENCE).await;

    unsubscribe(client, &subscription_id).await;
    scope.finish().await.expect("cleanup failed");
}

/// Table 5.3.2-1 gives every `CSourceNotification` member cardinality 1,
/// `triggerReason` included, and clause 5.3.3 says `newlyMatching` "is used in
/// the first notification". The periodic sender assembled its own body and
/// left the member out.
#[tokio::test]
async fn regression_csource_notification_a_periodic_notification_has_the_mandatory_members() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let registration_id = scope.id("ContextSourceRegistration:periodic-structure");
    let subscription_id = scope.id("Subscription:periodic-structure");

    register(client, &registration(&registration_id, "Building", None)).await;
    let mut subscription = csource_subscription(&subscription_id, "Building", &receiver.endpoint_url());
    subscription["timeInterval"] = json!(LONG_INTERVAL_SECONDS);
    subscribe(client, &subscription).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    for member in ["id", "type", "data", "notifiedAt", "subscriptionId", "triggerReason"] {
        assert!(
            notification.body.get(member).is_some(),
            "Table 5.3.2-1 gives '{member}' cardinality 1: {}",
            notification.body
        );
    }
    assert_eq!(notification.body["type"], json!("ContextSourceNotification"), "{}", notification.body);
    assert_eq!(notification.body["triggerReason"], json!("newlyMatching"), "{}", notification.body);

    unsubscribe(client, &subscription_id).await;
    deregister(client, &registration_id).await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.11.7 has the notification carry the registration itself, so its
/// entity types reach the subscriber compacted under the @context the
/// notification is sent with rather than in the expanded form the store holds.
#[tokio::test]
async fn regression_csource_notification_the_notified_registration_is_compacted() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let registration_id = scope.id("ContextSourceRegistration:compacted");
    let subscription_id = scope.id("Subscription:compacted");

    register(client, &registration(&registration_id, "Building", None)).await;
    subscribe(client, &csource_subscription(&subscription_id, "Building", &receiver.endpoint_url())).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    let reported = notified_entry(&notification.body, &registration_id, "the registration is reported");
    assert_eq!(
        reported["information"][0]["entities"][0]["type"],
        json!("Building"),
        "the notified registration is compacted, not expanded: {}",
        notification.body
    );

    unsubscribe(client, &subscription_id).await;
    deregister(client, &registration_id).await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.11.2.4: an `after` temporal query describes every instant from its
/// `timeAt` onwards, so it reaches a source whose `observationInterval` begins
/// later and still holds observations the query asks for.
#[tokio::test]
async fn regression_csource_notification_an_after_query_reaches_a_later_observation_interval() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let registration_id = scope.id("ContextSourceRegistration:after-observation");
    let subscription_id = scope.id("Subscription:after-observation");

    let mut subscription = csource_subscription(&subscription_id, "Building", &receiver.endpoint_url());
    subscription["temporalQ"] = json!({"timerel": "after", "timeAt": "2020-06-01T22:07:00Z", "timeproperty": "observedAt"});
    register(
        client,
        &registration(&registration_id, "Building", Some(("observationInterval", coverage_interval()))),
    )
    .await;
    subscribe(client, &subscription).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    assert_notified(
        &notification.body,
        &registration_id,
        "an interval beginning after the instant still answers for it",
    );

    unsubscribe(client, &subscription_id).await;
    deregister(client, &registration_id).await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.11.2.4: `createdAt` and `modifiedAt` read the
/// `managementInterval`, and a `before` query describes every instant up to
/// its `timeAt`, so it reaches a source whose interval ended earlier.
#[tokio::test]
async fn regression_csource_notification_a_before_query_reaches_an_earlier_management_interval() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let registration_id = scope.id("ContextSourceRegistration:before-management");
    let subscription_id = scope.id("Subscription:before-management");

    let mut subscription = csource_subscription(&subscription_id, "Building", &receiver.endpoint_url());
    subscription["temporalQ"] = json!({"timerel": "before", "timeAt": "2021-08-01T22:00:00Z", "timeproperty": "createdAt"});
    register(
        client,
        &registration(&registration_id, "Building", Some(("managementInterval", coverage_interval()))),
    )
    .await;
    subscribe(client, &subscription).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    assert_notified(
        &notification.body,
        &registration_id,
        "an interval ending before the instant still answers for it",
    );

    unsubscribe(client, &subscription_id).await;
    deregister(client, &registration_id).await;
    scope.finish().await.expect("cleanup failed");
}

/// The overlap is not a licence to match everything: clause 5.11.2.4 still
/// refuses a source whose interval ends before an `after` query begins.
#[tokio::test]
async fn regression_csource_notification_an_after_query_past_the_interval_end_matches_nothing() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let registration_id = scope.id("ContextSourceRegistration:after-disjoint");
    let subscription_id = scope.id("Subscription:after-disjoint");

    let mut subscription = csource_subscription(&subscription_id, "Building", &receiver.endpoint_url());
    subscription["temporalQ"] = json!({"timerel": "after", "timeAt": "2030-01-01T00:00:00Z", "timeproperty": "observedAt"});
    register(
        client,
        &registration(&registration_id, "Building", Some(("observationInterval", coverage_interval()))),
    )
    .await;
    subscribe(client, &subscription).await;

    receiver.wait_for_no_subscription_notification(&subscription_id, SILENCE).await;

    unsubscribe(client, &subscription_id).await;
    deregister(client, &registration_id).await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.10.2.4 matches a discovery's temporal query against the very same
/// intervals clause 5.11.2.4 matches a subscription's against, so the SQL the
/// discovery builds and the comparison the subscription runs have to agree.
#[tokio::test]
async fn regression_csource_discovery_a_temporal_query_reaches_a_partly_overlapping_interval() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let registration_id = scope.id("ContextSourceRegistration:discovery-overlap");

    register(
        client,
        &registration(&registration_id, "Building", Some(("observationInterval", coverage_interval()))),
    )
    .await;

    for (timerel, time_at, expectation) in [
        ("before", "2021-08-01T22:00:00Z", true),
        ("after", "2020-06-01T22:07:00Z", true),
        ("after", "2030-01-01T00:00:00Z", false),
        ("before", "2019-01-01T00:00:00Z", false),
    ] {
        let query = format!("type=Building&timerel={timerel}&timeAt={}&timeproperty=observedAt", encode(time_at));
        let listed = client.query_registrations(&query, Some(CORE_CONTEXT)).await.expect("query registrations");
        assert_eq!(listed.status, StatusCode::OK, "{timerel} {time_at}: {:?}", listed.body);
        let results = listed.json_value().expect("a registration query answers JSON");
        let found = results
            .as_array()
            .expect("a registration query answers an array")
            .iter()
            .any(|candidate| candidate["id"] == json!(registration_id));
        assert_eq!(found, expectation, "{timerel} {time_at} against 2020-08-01..2021-08-01: {results}");
    }

    deregister(client, &registration_id).await;
    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.12-2 makes a paused subscription one no notification is sent for,
/// and clause 5.11.7 sends only "if and only if the status ... is active", so
/// the schedule does not run for one either.
#[tokio::test]
async fn regression_csource_notification_a_paused_periodic_subscription_is_silent() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let registration_id = scope.id("ContextSourceRegistration:periodic-paused");
    let subscription_id = scope.id("Subscription:periodic-paused");

    register(client, &registration(&registration_id, "Building", None)).await;
    let mut subscription = csource_subscription(&subscription_id, "Building", &receiver.endpoint_url());
    subscription["timeInterval"] = json!(1);
    subscription["isActive"] = json!(false);
    subscribe(client, &subscription).await;

    receiver.wait_for_no_subscription_notification(&subscription_id, SILENCE).await;

    unsubscribe(client, &subscription_id).await;
    deregister(client, &registration_id).await;
    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.12-1 gives `throttling` cardinality 0 when `timeInterval` is
/// present, which is why the periodic sender passes no throttle to the
/// notification gate: the combination never reaches it.
#[tokio::test]
async fn regression_csource_notification_a_periodic_subscription_refuses_throttling() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let subscription_id = scope.id("Subscription:periodic-throttled");

    let mut subscription = csource_subscription(&subscription_id, "Building", "http://127.0.0.1:9/notify");
    subscription["timeInterval"] = json!(LONG_INTERVAL_SECONDS);
    subscription["throttling"] = json!(5);
    let created = create_csource_subscription(client, &subscription).await;

    assert_eq!(created.status, StatusCode::BAD_REQUEST, "create: {:?}", created.body);

    scope.finish().await.expect("cleanup failed");
}
