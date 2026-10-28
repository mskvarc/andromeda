//! ETSI `046_10`: a notification is sent as JSON, with the suite's `@context` linked in a Link header and not in the body
//! (clause 5.8.6).

use crate::{
    context_information::subscription::subscription_notification_behaviour::notification_case::{air_quality_updated, created, subscribed, teardown},
    suite_payloads::building_simple_attributes,
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, robot_literal, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use serde_json::Value;

/// Pins: The Notification shall be sent as JSON
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
/// Changed: Wait for notification hands the headers over as a plain dictionary (Convert To Dictionary of the received
/// message), keyed by the field names exactly as the broker spelled them, so the keyword looks the name up
/// case-sensitively; field names are case-insensitive (RFC 9110 clause 5.1), so the port checks the field whatever its
/// case, through a WireMock request pattern, which matches names without regard to case
/// Changed: Dictionary Should Contain Item reads Link from that dictionary, which holds only the first of several Link
/// field lines; the port requires exactly one Link line with the expected value (WireMock hasExactly), the field value the
/// keyword means, since RFC 9110 clause 5.3 makes several lines one comma-joined value
#[gtest]
#[tokio::test]
async fn e046_10_01_check_that_the_notification_is_sent_as_json() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory", "airQualityLevel", "almostFull"]).await?;
    let sink = NotificationSink::start().await?;
    let as_string = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let as_list = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let building = building_simple_attributes(&namespace, 1)?;
    created(&as_string, &building).await?;

    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).build()?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?).with_ids(building.id.clone())])
        .notification(NotificationParams::builder(endpoint).build()?)
        .build()?;
    let _subscription = subscribed(&as_list, &subscription).await?;

    air_quality_updated(&as_list, &building).await?;
    let notification = sink.wait_for(1, 1).await?;
    let body = robot_literal::read(&notification.body)?;
    expect_that!(body.pointer("/data/0"), some(predicate(Value::is_object)));
    expect_that!(body.pointer("/data/0/@context"), none());
    // The sink keeps no logged date, so the body identifies the journal entry the wait returned.
    let received = sink.received(1).await?;
    let bodies = received.into_iter().map(|entry| entry.body).collect::<Vec<_>>();
    expect_that!(bodies, contains(eq(&notification.body)));

    let expected_link = format!(
        "<{}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"",
        vocabulary.url().as_str()
    );
    let links: Vec<&str> = notification.headers.get_all("link").iter().filter_map(|value| value.to_str().ok()).collect();
    expect_that!(links, eq(&vec![expected_link.as_str()]));

    teardown(&broker, &id, &building).await;
    Ok(())
}
