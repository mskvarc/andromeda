//! A `temporalQ` on `observedAt` is matched against the `observationInterval` of a registration (clause 5.11.7).

use crate::{
    context_source::registration_subscription::context_source_registration_subscription_notification_behaviour::registration_notification_case::{
        expect_csource_notification,
        registered_any,
        teardown,
    },
    entity_operations::ld_json_body_client,
    registration_operations::csource_endpoint,
    registration_subscription_operations::create_registration_subscription,
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    query::{
        EntitySelector,
        temporal_query::{self, TemporalProperty},
    },
    registration::{CSourceRegistration, RegistrationId, RegistrationInfo, TimeInterval},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};

/// Pins: Check if a context source registration subscription defines temporalQ member with timeproperty observedAt,
/// the temporal query is matched against the observationInterval of matching context source registrations
/// Clause: 5.11.7
#[gtest]
#[tokio::test]
async fn e047_10_01_receive_csource_notification_for_matching_context_source_registrations_on_observation_interval() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let sink = NotificationSink::start().await?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let uri = sink.endpoint(1)?;
    let subscription = Subscription::registration_builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(NotificationParams::builder(Endpoint::builder(uri.as_str()).build()?).build()?)
        .temporal_q(
            temporal_query::after("2020-06-01T22:07:00Z".parse()?)
                .time_property(TemporalProperty::ObservedAt)
                .build()?,
        )
        .build()?;
    let _subscription = create_registration_subscription(&ld_json_body_client(&broker, &vocabulary)?, &subscription, &uri).await?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let registration_id: RegistrationId = namespace.urn_text("ContextSourceRegistration", 1).parse()?;
    let registration = CSourceRegistration::builder(endpoint.as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities(["Building"]).build()?])
        .observation_interval(TimeInterval::between("2020-08-01T22:07:00Z".parse()?, "2021-08-01T21:07:00Z".parse()?))
        .build()?;
    let _registration = registered_any(&broker, &vocabulary, &registration).await?;

    let notification = sink.wait_for(1, 1).await?;
    let body = notification.json()?;
    expect_csource_notification(&body, &id, &registration_id, "newlyMatching");

    teardown(&broker, &registration_id, &id).await
}
