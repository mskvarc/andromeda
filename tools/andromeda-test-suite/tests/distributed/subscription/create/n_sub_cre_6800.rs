//! A subscription without an id gets a generated uri in location.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, Location},
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription without an id answers 201 with a Location naming a generated URI, and Retrieve
/// Subscription on it reads the subscription back under that id
/// Clause: 5.8.1.4, 5.8.1.5, 5.2.12
#[gtest]
#[tokio::test]
async fn n_sub_cre_6800_a_subscription_without_an_id_gets_a_generated_uri_in_location() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let created = broker.with_responses().subscriptions().create(&subscription).send().await?;
    let location = created
        .head
        .typed::<Location>()
        .valid()
        .map(|location| location.as_iri_ref().as_str().to_owned());
    expect_that!(location, some(matches_regex("^/ngsi-ld/v1/subscriptions/urn:.+")));
    let subscription_id = created.outcome.id().or_fail()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(retrieved.id().to_string(), starts_with("urn:"));
    expect_that!(retrieved.id(), eq(&subscription_id));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
