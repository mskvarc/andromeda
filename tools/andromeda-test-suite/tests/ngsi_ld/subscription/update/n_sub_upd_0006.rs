//! Replaces the notification endpoint and format.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Representation, SimplifiedSpelling},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: a fragment carrying a notification with another format and endpoint is applied: the subscription reads
/// back with the new endpoint uri and accept and the new format
/// Clause: 5.8.2.4, 5.5.8, 5.2.14
#[gtest]
#[tokio::test]
async fn n_sub_upd_0006_replaces_the_notification_endpoint_and_format() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::JsonLd).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let replacement = NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?)
        .format(Representation::Simplified)
        .simplified_spelling(SimplifiedSpelling::KeyValues)
        .build()?;
    let fragment = SubscriptionFragment::builder().notification(replacement).build()?;
    let updated = broker.with_responses().subscriptions().update(&subscription_id, &fragment).send().await;
    expect_that!(&updated, answered(StatusCode::NO_CONTENT));
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    let notification = stored.notification();
    expect_that!(notification.format(), some(eq(Representation::Simplified)));
    expect_that!(notification.simplified_spelling(), eq(SimplifiedSpelling::KeyValues));
    expect_that!(notification.endpoint().uri(), eq(&sink.endpoint(2)?));
    expect_that!(notification.endpoint().accept(), some(eq(NgsiLdMediaType::Json)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
