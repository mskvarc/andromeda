//! A notification of a 20 mib entity reaches an http endpoint.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    eventually::retried,
    header_condition::HeaderCondition,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use http::header::CONTENT_LENGTH;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationStatus, NotificationTrigger, Subscription},
};

/// The octets of the description value the entity carries: 20 `MiB`.
const DESCRIPTION_OCTETS: usize = 20_971_520;

/// Pins: The creation of an Entity of 20 `MiB`, a body the broker accepts, is notified to an HTTP endpoint in one
/// notification carrying the Entity
/// Clause: 5.8.6, 5.2.15
#[gtest]
#[tokio::test]
async fn n_ntf_dlv_9004_a_notification_of_a_20_mib_entity_reaches_an_http_endpoint() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("description", "x".repeat(DESCRIPTION_OCTETS))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let notified = sink
        .wait_for_at_least_matching(
            1,
            &[BodyPattern::Selects(format!("$.data[?(@.id == '{building_id}')]").parse()?)],
            &[HeaderCondition::exactly(CONTENT_LENGTH, "2[0-9]{7}")?],
            1,
        )
        .await?;
    expect_that!(notified, len(eq(1)));
    retried(async || {
        let subscription = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await?;
        verify_that!(subscription.notification().output().status, some(eq(NotificationStatus::Ok)))?;
        verify_that!(subscription.notification().output().times_sent, some(eq(1)))
    })
    .await?;
    Ok(())
}
