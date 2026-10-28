//! ETSI `047_05`: a csource notification delivered to the endpoint updates the `notification` member of the
//! subscription (clause 5.11.7).

use crate::{
    context_source::registration_subscription::context_source_registration_subscription_notification_behaviour::registration_notification_case::{
        expect_delivery_state,
        registered_any,
        teardown,
    },
    suite_keywords::{create_registration_subscription, csource_endpoint, ld_json_body_client, once_delivery_recorded, registration_subscription_json},
    suite_payloads::{building_registration, building_registration_subscription},
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, robot_literal, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    model::NgsiLdMediaType,
    subscription::{Activation, SubscriptionId},
};

/// Pins: Check that if a cSourceNotification is sent successfully to the "endpoint" member, the
/// "notification.timesSent" member shall be incremented by one and the "notification.lastSuccess" and
/// "notification.lastNotification" members shall be updated with the current timestamp and the status of the context
/// source registration subscription shall be updated to "ok"
/// Clause: 5.11.7
/// Corrected: `fix(047-notification)`: register a matching context source and consume the initial notification (5.12)
/// Changed: Wait for notification parses the body as json.loads('''<body>'''), a Python '''...''' literal, so a
/// backslash in the body is read as an escape and a ''' or a final ' ends the literal; the port refuses all three (not
/// contains "\\", not contains "'''", not endsWith "'") and then requires the body to be JSON, which is stricter only
/// where an escape leaves the keyword reading, and accepting, a body other than the one sent
/// Changed: the Retrieve Context Source Registration Subscription after the notification is repeated every 250 ms for at most 10 s until its
/// `notification` records the delivery (`lastNotification` present), then checked: clause 5.11.7 records the delivery
/// once the endpoint's answer reaches the broker, which the endpoint's receipt of the notification precedes
#[gtest]
#[tokio::test]
async fn e047_05_01_if_a_csource_notification_is_successfully_sent_the_notification_member_shall_be_updated() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory", "locatedAt"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let subscription = building_registration_subscription(Some(id.clone()), &uri, Activation::Unspecified, None)?;
    let _subscription = create_registration_subscription(&ld_json_body_client(&broker, &vocabulary)?, &subscription, &uri).await?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let registration = building_registration(&namespace, 1, &endpoint)?;
    let registration_id = registration.id().or_fail()?;
    let _registration = registered_any(&broker, &vocabulary, &registration).await?;

    let notification = sink.wait_for(1, 1).await?;
    robot_literal::read(&notification.body)?;

    let reader = broker.with_accept(NgsiLdMediaType::Json);
    let body = once_delivery_recorded(|| registration_subscription_json(&reader, &id)).await?;
    expect_delivery_state(body, "ok", ["lastNotification", "lastSuccess"]);

    teardown(&broker, registration_id, &id).await
}
