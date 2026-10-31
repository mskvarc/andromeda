//! A null notification is bad request data.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::UpdateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};
use url::Url;

/// Pins: Update Subscription with notification null answers 400 `BadRequestData`, and the stored notification
/// endpoint stands
/// Clause: Table 5.2.12-1, 5.5.4, 5.8.3.4
#[gtest]
#[tokio::test]
async fn n_sub_upd_9005_a_null_notification_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_null_notification_id = namespace.subscription_id("null-notification")?;
    let expiry_probe_type = namespace.entity_type("ExpiryProbe")?;
    let subscription_null_notification = Subscription::builder()
        .id(subscription_null_notification_id.clone())
        .subscription_name("expiring")
        .entities([expiry_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .build()?;
    broker.subscriptions().create(&subscription_null_notification).send().await?;
    let fragment = SubscriptionFragment::builder().build()?;
    // The model refuses it (an unknown member notification), so `notification` is written as text.
    let notification: RawJson = "null".parse()?;
    let document = UncheckedDocument::try_from(fragment.render())?.set_member("notification", &notification);
    let request = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_null_notification_id,
            fragment: &fragment,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_null_notification_id)
        .send()
        .await?;
    expect_that!(
        subscription.notification().endpoint().uri(),
        eq(&format!("{unreachable}/notify").parse::<Url>()?)
    );
    broker.subscriptions().delete(&subscription_null_notification_id).send().await?;
    Ok(())
}
