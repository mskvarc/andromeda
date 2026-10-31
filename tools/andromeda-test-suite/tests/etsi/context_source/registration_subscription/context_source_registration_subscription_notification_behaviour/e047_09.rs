//! A registration whose information no longer matches the `entities` of a Context Source Registration Subscription is
//! notified as no longer matching (clause 5.11.7).

use crate::{
    context_source::registration_subscription::context_source_registration_subscription_notification_behaviour::registration_notification_case::{
        expect_csource_notification,
        registered,
        teardown,
    },
    entity_operations::ld_json_body_client,
    registration_operations::csource_endpoint,
    registration_payloads::building_registration,
    registration_subscription_operations::create_registration_subscription,
    subscription_payloads::building_registration_subscription,
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    registration::{RegistrationFragment, RegistrationInfo},
    subscription::{Activation, SubscriptionId},
};

/// Pins: Check if a context source registration subscription defines an "entities" member, a `CsourceNotification` will
/// be triggered from context source registrations with information member matching the described "entities"
/// Clause: 5.11.7
/// Expected: a matching context source is registered, so the subscription sends an initial notification, which is
/// consumed first (clause 5.12)
/// Expected: `NewType` stays out of the file vocabulary: the test reads it without a @context, so it expands under the
/// core default vocabulary
#[gtest]
#[tokio::test]
async fn e047_09_01_receive_csource_notification_for_no_longer_matching_context_source_registrations_providing_latest_information() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory", "locatedAt"]).await?;
    let sink = NotificationSink::start().await?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let registration = building_registration(&namespace, 1, &endpoint)?;
    let registration_id = registration.id().or_fail()?;
    let _registration = registered(&broker, &vocabulary, &registration).await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let subscription = building_registration_subscription(Some(id.clone()), &uri, Activation::Unspecified, None)?;
    let _subscription = create_registration_subscription(&ld_json_body_client(&broker, &vocabulary)?, &subscription, &uri).await?;
    let first = sink.wait_for(1, 1).await?;
    first.json()?;

    let fragment = RegistrationFragment::builder()
        .information([RegistrationInfo::builder().entities(["NewType"]).build()?])
        .build()?;
    broker.registrations().update(registration_id, &fragment).send().await.any_answer();

    let second = sink.wait_for(1, 2).await?;
    let body = second.json()?;
    expect_csource_notification(&body, &id, registration_id, "noLongerMatching");

    teardown(&broker, registration_id, &id).await
}
