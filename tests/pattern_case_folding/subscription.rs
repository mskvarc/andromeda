//! A subscription's `q` is answered by the in-memory engine (clause 5.8.1),
//! and it must select what the store selects for the same filter.

use crate::fixture::{CASE_FORMS, Spellings, encoded_filter, identifiers, notified_identifiers};
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

/// The filter both engines evaluate.
const FILTER: &str = r#"name~="(?i)^čebela$""#;

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

/// The subscription notifies exactly the Entities a query carrying the same
/// `q` selects: the four case forms, and neither the uncaroned word nor the
/// control.
#[tokio::test]
async fn case_folding_subscription_notifies_the_entities_the_same_filter_selects_in_the_store() {
    let scope = TestEnvironment::from_suite("pattern_case_folding").await.expect("isolated test environment");
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");
    let entity_type = scope.entity_type("SubscribedBee");

    let subscription = json!({
        "id": scope.id("Subscription:case-folding"),
        "type": "Subscription",
        "entities": [{"type": entity_type}],
        "notificationTrigger": ["entityCreated"],
        "q": FILTER,
        "notification": {"endpoint": {"uri": receiver.endpoint().expect("notification endpoint"), "accept": "application/json"}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = scope.client().create_subscription(&subscription).await.expect("create subscription request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let fixture = Spellings::create(&scope, "SubscribedBee").await;
    assert_eq!(fixture.entity_type, entity_type, "the fixture's type is the subscribed one");
    let expected = fixture.ids_of(&CASE_FORMS);

    let in_memory = notified(&receiver, expected.len()).await;
    let query = format!("type={entity_type}&q={}", encoded_filter(FILTER));
    let in_store = identifiers(
        &scope
            .client()
            .query_entities(&query, "application/json", Some(TEST_SUITE_CONTEXT))
            .await
            .expect("query request"),
    );

    assert_eq!(in_memory, expected, "the in-memory engine folds Č");
    assert_eq!(in_store, in_memory, "the store selects what the in-memory engine notifies");

    receiver.shutdown().await.expect("notification server shuts down");
    scope.finish().await.expect("cleanup failed");
}
