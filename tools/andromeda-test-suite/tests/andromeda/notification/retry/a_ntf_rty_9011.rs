//! A durable notification whose retry the endpoint's cooldown withholds is sent once the cooldown has passed.

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

/// The endpoint's `cooldown`, in milliseconds.
const COOLDOWN_MILLISECONDS: u32 = 2_000;

/// Well inside the cooldown and ten first retry steps of broker-single-tenant, so a retry that did not wait for the
/// cooldown would have arrived.
const INSIDE_THE_COOLDOWN: Duration = Duration::from_secs(1);

/// Pins: under durable delivery, a notification whose first attempt the endpoint answers with 503, and whose
/// endpoint has a `cooldown` of 2000 ms, is not attempted again within the cooldown although the first retry step is
/// 100 ms, and is attempted again once the cooldown has passed, under the same notification `id`
/// Clause: 5.2.15, 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery on broker-single-tenant, where a retry the
/// cooldown its own failed attempt opened withholds waits for the cooldown instead of being dropped
#[gtest]
#[tokio::test]
async fn a_ntf_rty_9011_a_retry_the_cooldown_withholds_is_sent_once_the_cooldown_has_passed() -> Result<()> {
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
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint(1)?)
                    .accept(NgsiLdMediaType::Json)
                    .cooldown(COOLDOWN_MILLISECONDS)
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    broker_single_tenant.subscriptions().create(&subscription).send().await?;
    let building = Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type(building_type.clone())
        .property("name", "a")
        .build()?;
    broker_single_tenant.entities().create(&building).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    expect_that!(sink.count_after(1, INSIDE_THE_COOLDOWN).await?, eq(1));
    let attempts = sink.wait_for_at_least(1, 2).await?;
    let first = attempts.first().or_fail()?.entities(Lossless::Normalized)?;
    let retry = attempts.get(1).or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(retry.id().to_string(), eq(&first.id().to_string()));
    broker_single_tenant.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
