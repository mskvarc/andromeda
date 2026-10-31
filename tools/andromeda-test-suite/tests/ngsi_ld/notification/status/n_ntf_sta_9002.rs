//! A refused notification is counted as sent and failed.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, notification_sink::NotificationSink, outcome::Done, source_answer};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, NotificationStatus, Subscription},
};

/// Pins: A notification answered 500 leaves the Subscription with `timesSent` and `timesFailed` of at least 1 and
/// status failed, where before it neither counter was present
/// Clause: Table 5.2.14.2-1, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_sta_9002_a_refused_notification_is_counted_as_sent_and_failed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_failing_id = namespace.urn("Building", "failing")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_failing_id = namespace.subscription_id("failing")?;
    sink.answer(
        "failing",
        source_answer::text(StatusCode::INTERNAL_SERVER_ERROR, HeaderValue::from_static("application/ld+json"), "{}"),
    )
    .await?;
    let building_failing = Entity::builder().id(building_failing_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building_failing).send().await?.done()?;
    let subscription_failing = Subscription::builder()
        .id(subscription_failing_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_failing_id.clone())])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("failing")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_failing).send().await?;
    let subscription = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_failing_id)
        .send()
        .await?;
    expect_that!(subscription.notification().output().times_failed, none());
    expect_that!(subscription.notification().output().times_sent, none());
    let fragment = EntityFragment::builder().property("note", "failing").build()?;
    broker.entities().append_attributes(&building_failing_id, &fragment).send().await?.done()?;
    sink.wait_for_at_least("failing", 1).await?;
    retried(async || {
        let subscription_2 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_failing_id)
            .send()
            .await?;
        verify_that!(subscription_2.notification().output().times_failed, some(ge(1)))?;
        verify_that!(subscription_2.notification().output().times_sent, some(ge(1)))?;
        verify_that!(subscription_2.notification().output().status, some(eq(NotificationStatus::Failed)))?;
        Ok(())
    })
    .await?;
    broker.subscriptions().delete(&subscription_failing_id).send().await?;
    Ok(())
}
