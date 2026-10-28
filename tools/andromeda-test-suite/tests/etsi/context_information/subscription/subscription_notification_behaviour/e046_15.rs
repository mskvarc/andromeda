//! ETSI `046_15`: a subscription with a `throttling` member sends no notification before the throttling has elapsed (clause
//! 5.8.6).

use crate::{
    context_information::subscription::subscription_notification_behaviour::notification_case::{air_quality_updated, created, teardown},
    suite_keywords::{create_subscription_document, simplified_notification},
    suite_payloads::{building_simple_attributes, key_values_document},
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, robot_literal, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    query::EntitySelector,
    subscription::{Subscription, SubscriptionId},
};
use std::time::Duration;
use tokio::time::sleep;

/// Pins: If a Subscription defines a throttling member, a Notification shall not be sent if the throttling specified (in seconds)
/// has not elapsed yet.
/// Clause: 5.8.6
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a backslash
/// in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not contains
/// "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only where an
/// escape leaves the keyword reading, and accepting, a body other than the one sent
/// Absence: Wait for no notification 5 s
#[gtest]
#[tokio::test]
async fn e046_15_01_check_that_a_notification_is_not_sent_if_the_throttling_has_not_elapsed_yet() -> Result<()> {
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
    let endpoint_url = sink.endpoint(1)?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?).with_ids(building.id.clone())])
        .throttling(15)
        .notification(simplified_notification(&endpoint_url)?)
        .build()?;
    let _subscription = create_subscription_document(&as_list, &subscription, &key_values_document(&subscription, &endpoint_url)?).await?;
    sleep(Duration::from_secs(1)).await;

    air_quality_updated(&as_list, &building).await?;
    let notification = sink.wait_for(1, 1).await?;
    robot_literal::read(&notification.body)?;

    air_quality_updated(&as_list, &building).await?;
    let total = sink.count_after(1, Duration::from_secs(5)).await?;
    expect_that!(total, eq(1));

    teardown(&broker, &id, &building).await;
    Ok(())
}
