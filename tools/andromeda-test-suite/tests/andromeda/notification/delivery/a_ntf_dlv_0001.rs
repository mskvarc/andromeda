//! A durable notification queued before its Subscription stopped selecting the change is still sent.

use andromeda_test_suite::{
    attribute_matcher::property,
    body_pattern::BodyPattern,
    broker,
    config::BrokerRole,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType, Representation},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};
use std::time::Duration;
use wiremock::{
    Mock,
    ResponseTemplate,
    matchers::{method, path},
};

/// Pins: under durable delivery a `speed` update whose first delivery failed, and whose Subscription is then updated
/// to select Trucks, watch `heading` and require `speed>100` before the retry, is still notified, exactly once, with
/// the `speed` the update wrote
/// Clause: 5.8.2, 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable), where a notification owed at commit waits in a queue while its Subscription may still be updated
#[gtest]
#[tokio::test]
async fn a_ntf_dlv_0001_a_durable_notification_queued_before_its_subscription_stopped_selecting_the_change_is_still_sent() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let truck_type = namespace.entity_type("Truck")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let subscription_id = namespace.subscription_id(1)?;
    let written = [BodyPattern::Contains("cruising".to_owned())];
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 0)
        .build()?;
    broker_durable.entities().create(&vehicle).send().await?.done()?;
    sink.mount(
        Mock::given(method("POST"))
            .and(path("/notify/1"))
            .respond_with(ResponseTemplate::new(500))
            .up_to_n_times(1)
            .with_priority(1),
    )
    .await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .watched_attributes(["speed"])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker_durable.subscriptions().create(&subscription).send().await?;
    let fragment = EntityFragment::builder().property("speed", "cruising").build()?;
    broker_durable.entities().update_attributes(&vehicle_id, &fragment).send().await?.done()?;
    sink.wait_for_at_least_where(1, &written, 1).await?;
    // The first retry comes two seconds after the failed attempt, so the update lands while the notification waits.
    let reselected = SubscriptionFragment::builder()
        .entities([truck_type.as_str()])
        .watched_attributes(["heading"])
        .q(q::attr("speed").gt(100).build()?)
        .build()?;
    broker_durable.subscriptions().update(&subscription_id, &reselected).send().await?;
    let attempts = sink.wait_for_at_least_where(1, &written, 2).await?;
    let delivered = attempts.get(1).or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        delivered.data().as_slice(),
        nth(0, all!(has_id(&vehicle_id), has_attribute("speed", property().value("cruising"))))
    );
    expect_that!(sink.count_after(1, Duration::from_secs(3)).await?, eq(2));
    broker_durable.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
