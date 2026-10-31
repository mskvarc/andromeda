//! A notification outlasting the endpoint timeout is a failure.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, notification_sink::NotificationSink, outcome::Done, source_answer};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, NotificationStatus, Subscription},
};
use std::time::Duration;

/// Pins: A notification to an endpoint with timeout 300 that answers only after five seconds leaves the
/// Subscription with status failed and a `lastFailure`
/// Clause: Table 5.2.14.2-1, Table 5.2.15-1, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_sta_9001_a_notification_outlasting_the_endpoint_timeout_is_a_failure() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_timeout_id = namespace.urn("Building", "timeout")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_timeout_id = namespace.subscription_id("timeout")?;
    sink.answer("timeout", source_answer::status(StatusCode::NO_CONTENT).set_delay(Duration::from_millis(5000)))
        .await?;
    let building_timeout = Entity::builder().id(building_timeout_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building_timeout).send().await?.done()?;
    let subscription_timeout = Subscription::builder()
        .id(subscription_timeout_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_timeout_id.clone())])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(sink.endpoint("timeout")?)
                    .accept(NgsiLdMediaType::Json)
                    .timeout(300)
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_timeout).send().await?;
    let fragment = EntityFragment::builder().property("note", "timeout").build()?;
    broker.entities().append_attributes(&building_timeout_id, &fragment).send().await?.done()?;
    sink.wait_for_at_least("timeout", 1).await?;
    retried(async || {
        let subscription = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_timeout_id)
            .send()
            .await?;
        verify_that!(subscription.notification().output().status, some(eq(NotificationStatus::Failed)))?;
        verify_that!(subscription.notification().output().last_failure, some(anything()))?;
        Ok(())
    })
    .await?;
    broker.subscriptions().delete(&subscription_timeout_id).send().await?;
    Ok(())
}
