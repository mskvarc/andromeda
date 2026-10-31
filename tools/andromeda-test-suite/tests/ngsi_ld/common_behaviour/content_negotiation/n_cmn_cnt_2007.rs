//! Create context source subscription declaring merge patch json is 415.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::answered, wire::content_type};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::{Accept, ContentType},
        operation::CreateSubscription,
    },
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Context Source Registration Subscription (POST) declaring application/merge-patch+json is refused
/// with a bare 415 and stores nothing
/// Clause: 6.3.4, 5.11.2
/// Recorded: 6.3.4 allows application/merge-patch+json for PATCH requests only; any other media type than
/// application/json and application/ld+json is 415 without a body
#[gtest]
#[tokio::test]
async fn n_cmn_cnt_2007_create_context_source_subscription_declaring_merge_patch_json_is_415() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let device_type = namespace.entity_type("Device")?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("s1")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The body goes as application/merge-patch+json, which no typed operation sends, so the media type is replaced.
    let request = content_type(
        broker.unchecked(&CreateSubscription { subscription: &subscription })?,
        &ContentType::MergePatch.media_type(),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE).without_body());
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
