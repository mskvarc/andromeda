//! A subscription's `q` is answered by the in-memory engine (clause 5.8.1),
//! and it must select what the store selects for the same filter.

use crate::fixture::{CASE_FORMS, ONE_WORD, Spellings, encoded_filter, identifiers, notified_identifiers};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{notification_server::NotificationServer, server_config::MockServerConfig};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};

/// How long the first notifications are waited for.
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a further notification is waited for once every expected one has
/// arrived, so an Entity that must not be notified has had its chance to be.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(3);

/// Every Entity the receiver was notified of, read until `expected` have
/// arrived and then once more.
async fn notified(receiver: &NotificationServer, expected: usize) -> BTreeSet<String> {
    let mut notified = BTreeSet::new();
    loop {
        let timeout = if notified.len() < expected { DELIVERY_TIMEOUT } else { SETTLE_TIMEOUT };
        let Ok(captured) = receiver.next_request(timeout).await else {
            return notified;
        };
        let body: Value = captured.json().expect("a notification body is JSON");
        notified.extend(notified_identifiers(&body));
    }
}

/// Subscribe with `filter`, create the fixture, and assert the subscription
/// notifies exactly the Entities holding `expected`, which is also what a query
/// carrying the same `q` selects.
async fn assert_notified_as_selected(base_type: &str, filter: &str, expected: &[&str]) {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");
    let entity_type = scope.entity_type(base_type);

    let subscription = json!({
        "id": scope.id("Subscription:unicode-pattern"),
        "type": "Subscription",
        "entities": [{"type": entity_type}],
        "notificationTrigger": ["entityCreated"],
        "q": filter,
        "notification": {"endpoint": {"uri": receiver.endpoint().expect("notification endpoint"), "accept": "application/json"}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = scope.client().create_subscription(&subscription).await.expect("create subscription request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let fixture = Spellings::create(&scope, base_type).await;
    assert_eq!(fixture.entity_type, entity_type, "the fixture's type is the subscribed one");
    let expected = fixture.ids_of(expected);

    let in_memory = notified(&receiver, expected.len()).await;
    let query = format!("type={entity_type}&q={}", encoded_filter(filter));
    let in_store = identifiers(
        &scope
            .client()
            .query_entities(&query, "application/json", Some(TEST_SUITE_CONTEXT))
            .await
            .expect("query request"),
    );

    assert_eq!(in_memory, expected, "the in-memory engine answers {filter}");
    assert_eq!(in_store, in_memory, "the store selects what the in-memory engine notifies for {filter}");

    receiver.shutdown().await.expect("notification server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// The subscription notifies exactly the Entities a query carrying the same
/// `q` selects: the four case forms, and neither the uncaroned word nor the
/// control.
#[tokio::test]
async fn unicode_pattern_subscription_notifies_the_entities_the_same_filter_selects_in_the_store() {
    assert_notified_as_selected("SubscribedBee", r#"name~="(?i)^čebela$""#, &CASE_FORMS).await;
}

/// A word class answered in memory holds `č` as the store's restatement does:
/// the one-word spellings are notified and the place name is not.
#[tokio::test]
async fn unicode_pattern_subscription_a_word_class_notifies_the_entities_the_store_selects() {
    assert_notified_as_selected("SubscribedWordBee", r#"name~="^\w+$""#, &ONE_WORD).await;
}
