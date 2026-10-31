//! A Context Source Registration Subscription with a `timeInterval` notifies initially on subscription (clause 5.11.7).

use crate::{
    context_source::registration_subscription::context_source_registration_subscription_notification_behaviour::registration_notification_case::{
        registered,
        teardown,
    },
    entity_operations::ld_json_body_client,
    registration_operations::csource_endpoint,
    registration_payloads::vehicle_parking_registration,
    registration_subscription_operations::registration_subscription_request,
    subscription_payloads::key_values_document,
};
use andromeda_test_suite::{broker, cleanup::Cleanup, namespace::Namespace, notification_sink::NotificationSink, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};

/// Pins: Check that if the created context source registration subscription defines a timeInterval member, a
/// cSourceNotification will be sent periodically, initially on subscription and when the time interval is reached
/// Clause: 5.11.7
/// Isolation: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it
/// matches only the test's own entities
#[gtest]
#[tokio::test]
async fn e047_01_01_receive_csource_notification_periodically_and_initially_on_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(
        &broker,
        &namespace,
        &[
            "Vehicle",
            "brandName",
            "speed",
            "isParked",
            "OffStreetParking",
            "availableSpotNumber",
            "totalSpotNumber",
            "isNextToBuilding",
            "Building",
        ],
    )
    .await?;
    let sink = NotificationSink::start().await?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let registration = vehicle_parking_registration(&namespace, &endpoint, namespace.urn("Vehicle", "A456")?, None)?;
    let _registration = registered(&broker, &vocabulary, &registration).await?;

    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let subscription = Subscription::registration_builder()
        .id(id.clone())
        .time_interval(10)
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(NotificationParams::builder(Endpoint::builder(uri.as_str()).build()?).build()?)
        .build()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _subscription = Cleanup::of(&client).registration_subscription_of(&subscription);
    let request = registration_subscription_request(&client, &subscription, &key_values_document(&subscription, &uri)?)?;
    client.send_unchecked(request).await.any_answer();

    let notification = sink.wait_for(1, 1).await?;
    notification.json()?;

    teardown(&broker, registration.id().or_fail()?, &id).await
}
