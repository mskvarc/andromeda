//! A subscription past its expires at reads back expired while one without stays active.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, notification_sink::NotificationSink};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionStatus},
};

/// Pins: a Context Source Registration Subscription created with an `expiresAt` three seconds ahead reads back with
/// status expired once that instant has passed, while one created without an `expiresAt` still reads back active
/// Clause: 5.11.2.4, 5.2.12
#[gtest]
#[tokio::test]
async fn n_css_cre_0004_a_subscription_past_its_expires_at_reads_back_expired_while_one_without_stays_active() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_1_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let plus_3: DateTime = (Utc::now() + TimeDelta::seconds(3)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let subscription_1 = Subscription::registration_builder()
        .id(subscription_1_id.clone())
        .entities([building_type.as_str()])
        .expires_at(plus_3)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Simplified)
                .build()?,
        )
        .build()?;
    broker.registration_subscriptions().create(&subscription_1).send().await?;
    let subscription_2 = Subscription::registration_builder()
        .id(subscription_2_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Simplified)
                .build()?,
        )
        .build()?;
    broker.registration_subscriptions().create(&subscription_2).send().await?;
    retried(async || {
        let expired = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .registration_subscriptions()
            .retrieve(&subscription_1_id)
            .send()
            .await?;
        verify_that!(expired.expires_at(), some(eq(&plus_3)))?;
        verify_that!(expired.output().status, some(eq(SubscriptionStatus::Expired)))?;
        Ok(())
    })
    .await?;
    let active = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await?;
    expect_that!(active.output().status, some(eq(SubscriptionStatus::Active)));
    broker.registration_subscriptions().delete(&subscription_1_id).send().await?;
    broker.registration_subscriptions().delete(&subscription_2_id).send().await?;
    Ok(())
}
