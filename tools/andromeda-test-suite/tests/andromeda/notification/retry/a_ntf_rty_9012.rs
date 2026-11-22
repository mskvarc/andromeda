//! A durable notification whose retry its Subscription's throttling withholds is sent once the window has passed.

use andromeda_test_suite::{broker, config::BrokerRole, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use std::time::Duration;
use wiremock::{
    Mock,
    ResponseTemplate,
    matchers::{method, path},
};

/// The Subscription's `throttling`, in seconds.
const THROTTLING_SECONDS: u32 = 2;

/// Well inside the throttling window and ten first retry steps of broker-single-tenant, so a retry that did not wait
/// for the window would have arrived.
const INSIDE_THE_WINDOW: Duration = Duration::from_secs(1);

/// Pins: under durable delivery, a notification of a Subscription with a `throttling` of 2 seconds whose first attempt
/// the endpoint answers with 503 is not attempted again within the window although the first retry step is 100 ms,
/// and is attempted again once the window has passed, under the same notification `id`
/// Clause: 5.2.12, 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery on broker-single-tenant, where a retry the
/// throttling window its own failed attempt opened withholds waits for the window instead of being dropped
#[gtest]
#[tokio::test]
async fn a_ntf_rty_9012_a_retry_the_throttling_withholds_is_sent_once_the_window_has_passed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_single_tenant = broker::client(BrokerRole::SingleTenant)?;
    let sink = NotificationSink::start().await?;
    sink.mount(
        Mock::given(method("POST"))
            .and(path("/notify/1"))
            .respond_with(ResponseTemplate::new(503))
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
        .throttling(THROTTLING_SECONDS)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker_single_tenant.subscriptions().create(&subscription).send().await?;
    let building = Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type(building_type.clone())
        .property("name", "a")
        .build()?;
    broker_single_tenant.entities().create(&building).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    expect_that!(sink.count_after(1, INSIDE_THE_WINDOW).await?, eq(1));
    let attempts = sink.wait_for_at_least(1, 2).await?;
    let first = attempts.first().or_fail()?.entities(Lossless::Normalized)?;
    let retry = attempts.get(1).or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(retry.id().to_string(), eq(&first.id().to_string()));
    broker_single_tenant.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
