//! A subscription without an id gets a subscription urn.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, Location},
    model::{NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Context Source Registration Subscription without an id answers 201 with a Location naming a
/// generated urn:ngsi-ld:Subscription: identifier, under which the subscription is then retrievable
/// Clause: 5.11.2.4, 6.10.3.1
/// Recorded: 5.11.2.4 leaves the form of a generated identifier to the broker; Andromeda generates a
/// urn:ngsi-ld:Subscription: URN
#[gtest]
#[tokio::test]
async fn o_css_cre_0001_a_subscription_without_an_id_gets_a_subscription_urn() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::registration_builder()
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Simplified)
                .build()?,
        )
        .build()?;
    let created = broker.with_responses().registration_subscriptions().create(&subscription).send().await?;
    let location = created
        .head
        .typed::<Location>()
        .valid()
        .map(|location| location.as_iri_ref().as_str().to_owned());
    expect_that!(location, some(matches_regex("^/ngsi-ld/v1/csourceSubscriptions/urn:ngsi-ld:Subscription:.+$")));
    let subscription_id = created.outcome.id().or_fail()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(retrieved.id().to_string(), matches_regex("^urn:ngsi-ld:Subscription:.+$"));
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
