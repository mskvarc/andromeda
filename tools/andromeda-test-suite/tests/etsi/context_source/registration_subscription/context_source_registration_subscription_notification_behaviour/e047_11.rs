//! A `temporalQ` on `createdAt` or `modifiedAt` is matched against the `managementInterval` of a registration (clause
//! 5.11.7).

use crate::{
    context_source::registration_subscription::context_source_registration_subscription_notification_behaviour::registration_notification_case::{
        expect_csource_notification,
        registered,
    },
    entity_operations::ld_json_body_client,
    registration_operations::csource_endpoint,
    registration_subscription_operations::create_registration_subscription,
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    query::{
        EntitySelector,
        temporal_query::{self, TemporalProperty},
    },
    registration::{CSourceRegistration, RegistrationId, RegistrationInfo, TimeInterval},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};

/// Pins: Check if a context source registration subscription defines temporalQ member with timeproperty createdAt or
/// modifiedAt, the temporal query is matched against the managementInterval of matching context source registrations
/// Clause: 5.11.7
#[gtest]
#[tokio::test]
async fn e047_11_01_created_at() -> Result<()> {
    matched_on_management_interval(TemporalProperty::CreatedAt).await
}

/// Pins: Check if a context source registration subscription defines temporalQ member with timeproperty createdAt or
/// modifiedAt, the temporal query is matched against the managementInterval of matching context source registrations
/// Clause: 5.11.7
#[gtest]
#[tokio::test]
async fn e047_11_02_modified_at() -> Result<()> {
    matched_on_management_interval(TemporalProperty::ModifiedAt).await
}

/// Subscribes to every Building with a `temporalQ` after 2020-06-01T22:07:00Z on `property` (201), registers a
/// Building source managed from 2020-08-01T22:07:00Z to 2021-08-01T21:07:00Z (201), requires its newly matching
/// notification within 5 s, then deletes the subscription whatever the answer and the registration, requiring a status
/// below 400.
async fn matched_on_management_interval(property: TemporalProperty) -> Result<()> {
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
        .temporal_q(temporal_query::after("2020-06-01T22:07:00Z".parse()?).time_property(property).build()?)
        .build()?;
    let _subscription = create_registration_subscription(&ld_json_body_client(&broker, &vocabulary)?, &subscription, &uri).await?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let registration_id: RegistrationId = namespace.urn_text("ContextSourceRegistration", 1).parse()?;
    let registration = CSourceRegistration::builder(endpoint.as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities(["Building"]).build()?])
        .management_interval(TimeInterval::between("2020-08-01T22:07:00Z".parse()?, "2021-08-01T21:07:00Z".parse()?))
        .build()?;
    let _registration = registered(&broker, &vocabulary, &registration).await?;

    let notification = sink.wait_for(1, 1).await?;
    let body = notification.json()?;
    expect_csource_notification(&body, &id, &registration_id, "newlyMatching");

    broker.registration_subscriptions().delete(&id).send().await.any_answer();
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
