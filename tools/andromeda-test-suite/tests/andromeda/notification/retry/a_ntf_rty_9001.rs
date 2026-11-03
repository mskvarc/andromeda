//! A durable notification attempted again after a retryable failure reaches the endpoint with the same id.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    namespace::Namespace,
    notification_sink::NotificationSink,
    notification_uuid::uuid_version,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use uuid::Version;
use wiremock::{
    Mock,
    ResponseTemplate,
    matchers::{method, path},
};

/// Pins: under durable delivery, a notification whose first attempt the endpoint answers with 500 and whose retry it
/// accepts reaches the endpoint twice under one notification `id`, a name-based (version 5) UUID
/// Clause: 5.3.1, 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable), where every attempt of one queued notification carries the same `id`
#[gtest]
#[tokio::test]
async fn a_ntf_rty_9001_a_durable_notification_attempted_again_after_a_retryable_failure_keeps_its_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let sink = NotificationSink::start().await?;
    sink.mount(
        Mock::given(method("POST"))
            .and(path("/notify/1"))
            .respond_with(ResponseTemplate::new(500))
            .up_to_n_times(1)
            .with_priority(1),
    )
    .await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker_durable.subscriptions().create(&subscription).send().await?;
    let entity = Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type(building_type.clone())
        .property("name", "a")
        .build()?;
    broker_durable.entities().create(&entity).send().await?.done()?;
    let attempts = sink.wait_for_at_least(1, 2).await?;
    let first = attempts.first().or_fail()?.entities(Lossless::Normalized)?;
    let second = attempts.get(1).or_fail()?.entities(Lossless::Normalized)?;
    let first_id = first.id().to_string();
    expect_that!(second.id().to_string(), eq(&first_id));
    expect_that!(uuid_version(first.id()), some(eq(Version::Sha1)));
    broker_durable.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
