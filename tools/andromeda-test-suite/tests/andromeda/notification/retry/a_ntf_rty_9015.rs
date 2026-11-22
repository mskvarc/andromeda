//! A durable notification its endpoint answers 503 with `Retry-After` is attempted again only once the wait it asked
//! for has passed.

use andromeda_test_suite::{broker, config::BrokerRole, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use http::{HeaderValue, header::RETRY_AFTER};
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use std::time::Duration;
use tokio::time::{Instant, sleep_until};
use wiremock::{
    Mock,
    ResponseTemplate,
    matchers::{method, path},
};

/// The wait the endpoint asks for, in seconds: the 1-second longest retry step of broker-single-tenant, so the whole
/// wait is honoured.
const REQUESTED_SECONDS: u64 = 1;

/// Well inside the requested wait and eight first retry steps of broker-single-tenant, so a retry that did not wait as
/// asked would have arrived.
const INSIDE_THE_REQUESTED_WAIT: Duration = Duration::from_millis(800);

/// Pins: under durable delivery, a notification whose first attempt the endpoint answers with 503 and
/// `Retry-After: 1` is not attempted again within 800 ms of its change although the first retry step is 100 ms, and is
/// attempted again after that, under the same notification `id`
/// Clause: 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery on broker-single-tenant, where a retry
/// waits at least as long as the endpoint's `Retry-After` asks (RFC 9110 section 10.2.3), up to the longest retry step
#[gtest]
#[tokio::test]
async fn a_ntf_rty_9015_a_retry_waits_as_long_as_the_endpoint_asked_with_retry_after() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_single_tenant = broker::client(BrokerRole::SingleTenant)?;
    let sink = NotificationSink::start().await?;
    sink.mount(
        Mock::given(method("POST"))
            .and(path("/notify/1"))
            .respond_with(ResponseTemplate::new(503).insert_header(RETRY_AFTER, HeaderValue::from(REQUESTED_SECONDS)))
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
    broker_single_tenant.subscriptions().create(&subscription).send().await?;
    let building = Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type(building_type.clone())
        .property("name", "a")
        .build()?;
    // Taken before the change is written, so the first attempt and the failure the requested wait counts from both come
    // later, and the absence check holds however late the sink's poll sees the first attempt.
    let written_from = Instant::now();
    broker_single_tenant.entities().create(&building).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    sleep_until(written_from + INSIDE_THE_REQUESTED_WAIT).await;
    expect_that!(sink.received(1).await?, len(eq(1)));
    let attempts = sink.wait_for_at_least(1, 2).await?;
    let first = attempts.first().or_fail()?.entities(Lossless::Normalized)?;
    let retry = attempts.get(1).or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(retry.id().to_string(), eq(&first.id().to_string()));
    broker_single_tenant.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
