//! A subscription answers 201 at its id and one without an id gets a uri.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, Location},
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Context Source Registration Subscription on broker A with an explicit id answers 201 with a
/// Location ending in it and the subscription reads back; without an id it answers 201 with a Location naming a URI
/// Clause: 5.11.2.4, 5.2.12
#[gtest]
#[tokio::test]
async fn n_css_cre_7600_a_subscription_answers_201_at_its_id_and_one_without_an_id_gets_a_uri() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let created = broker.with_responses().registration_subscriptions().create(&subscription).send().await?;
    let location = created
        .head
        .typed::<Location>()
        .valid()
        .map(|location| location.as_iri_ref().as_str().to_owned());
    expect_that!(
        location,
        some(ends_with(format!("/ngsi-ld/v1/csourceSubscriptions/urn:ngsi-ld:Subscription:{namespace}-1")))
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(retrieved.id(), eq(&subscription_id));
    let subscription_2 = Subscription::registration_builder()
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let created_2 = broker.with_responses().registration_subscriptions().create(&subscription_2).send().await?;
    let location_2 = created_2
        .head
        .typed::<Location>()
        .valid()
        .map(|location| location.as_iri_ref().as_str().to_owned());
    expect_that!(
        location_2,
        some(contains_regex("/ngsi-ld/v1/csourceSubscriptions/[A-Za-z][A-Za-z0-9+.-]*:[^/]+$"))
    );
    let generated_id = created_2.outcome.id().or_fail()?;
    broker.registration_subscriptions().delete(&generated_id).send().await?;
    Ok(())
}
