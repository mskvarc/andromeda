//! A Subscription whose q the release stored with a dot-free IRI reads back as the release wrote it and still notifies
//! once the hub is upgraded.

use andromeda_test_suite::{
    body_json::json_equals,
    broker,
    broker_process::BrokerProcess,
    config::BrokerRole,
    hosted_context::HostedContext,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    received_subscription::retrieved_as_received,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, Entity, InlineContext, Lossless, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, EntitySubscription, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: a Subscription the hub created on v1.0.0 with q=speed>50 under an @context mapping speed to
/// urn:<namespace>:speed, which v1.0.0 retrieves with no Link header as the unquoted urn:<namespace>:speed>50, is
/// retrieved once the hub runs the working tree under that @context with speed>50 and with no Link header with
/// urn:<namespace>:speed>50, and notifies for the creation of a Car whose speed is 80
/// Clause: 4.9, 5.5.7, 5.8.3, 5.8.6
/// Recorded: v1.0.0 wrote a name unquoted unless it held a dot, a space or a quote; the upgrade quotes every stored
/// name holding a colon, and this build writes a name holding no dot back unquoted outside the head of a hop's braces
#[gtest]
#[tokio::test]
async fn a_upg_qnm_0001_a_subscription_q_the_release_stored_with_a_dot_free_iri_reads_back_as_written_and_still_notifies() -> Result<()> {
    let namespace = Namespace::fresh();
    let hub = broker::client(BrokerRole::NamesHub)?;
    let sink = NotificationSink::start().await?;
    let iri = format!("urn:{namespace}:speed");
    let termed = HostedContext::add(
        &hub,
        &Context::Single(ContextEntry::Inline(format!(r#"{{"speed":"{iri}"}}"#).parse::<InlineContext>()?)),
    )
    .await?;
    let writer = hub.with_context(Context::Single(termed.entry()?))?;
    let car_type = namespace.entity_type("Car")?;
    let subscription_id = namespace.subscription_id(1)?;
    let speed_above_50 = q::attr("speed").gt(50).build()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([car_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .q(speed_above_50.clone())
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    writer.subscriptions().create(&subscription).send().await?;
    // The q model reads a name only as an unquoted AttrName, so a q naming the IRI is read as received.
    let unquoted_iri = format!("{iri}>50");
    let released = retrieved_as_received(&hub, &subscription_id, EntitySubscription).await?;
    expect_that!(released.body(), json_equals("$.q", unquoted_iri.as_str()));

    BrokerProcess::of(BrokerRole::NamesHub)?.upgrade().await?;
    let retrieved = writer
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(retrieved.q(), some(eq(&speed_above_50)));
    let received = retrieved_as_received(&hub, &subscription_id, EntitySubscription).await?;
    expect_that!(received.body(), json_equals("$.q", unquoted_iri.as_str()));
    let fast_id = namespace.urn("Car", "fast")?;
    let fast = Entity::builder().id(fast_id.clone()).entity_type(car_type).property("speed", 80).build()?;
    writer.entities().create(&fast).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&fast_id)));
    Ok(())
}
