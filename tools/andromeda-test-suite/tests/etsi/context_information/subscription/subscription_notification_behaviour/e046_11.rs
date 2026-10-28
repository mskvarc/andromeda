//! ETSI `046_11`: the `timesSent` of a subscription counts its notifications, and the first notification sets it to 1
//! (clause 5.8.6).

use crate::{
    context_information::subscription::subscription_notification_behaviour::notification_case::{air_quality_updated, created, subscription_created, teardown},
    suite_keywords::once_delivery_recorded,
    suite_payloads::building_simple_attributes,
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, robot_literal, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{NgsiLdMediaType, Representation},
    query::EntitySelector,
    subscription::{Activation, Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use serde_json::json;

/// Pins: The notification.timesSent member shall be incremented by one.
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
/// Changed: the Retrieve Subscription after the notification is repeated every 250 ms for at most 10 s until its
/// `notification` records the delivery (`lastNotification` present), then checked: clause 5.8.6 records the delivery
/// once the endpoint's answer reaches the broker, which the endpoint's receipt of the notification precedes
#[gtest]
#[tokio::test]
async fn e046_11_01_check_that_times_sent_is_increased_by_one() -> Result<()> {
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
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).accept(NgsiLdMediaType::Json).build()?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .activation(Activation::Active)
        .entities([EntitySelector::new("Building".parse()?).with_ids(building.id.clone())])
        .notification(NotificationParams::builder(endpoint).format(Representation::Normalized).build()?)
        .build()?;
    let _subscription = subscription_created(&as_list, &subscription).await?;

    air_quality_updated(&as_list, &building).await?;
    let notification = sink.wait_for(1, 1).await?;
    robot_literal::read(&notification.body)?;

    let reader = broker.with_context(vocabulary.context())?.with_accept(Accept::new(NgsiLdMediaType::JsonLd));
    let retrieved = once_delivery_recorded(|| async { Ok(json_body(&reader.with_responses().subscriptions().retrieve(&id).send().await?)?) }).await?;
    expect_that!(retrieved.pointer("/notification/timesSent"), some(any!(eq(&json!(1)), eq(&json!(true)))));

    teardown(&broker, &id, &building).await;
    Ok(())
}
