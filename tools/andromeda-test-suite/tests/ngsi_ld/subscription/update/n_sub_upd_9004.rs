//! An ngsi ld null expires at makes the subscription perpetual.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::UpdateSubscription},
    model::{DateTime, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment, SubscriptionStatus},
};

/// Pins: Update Subscription with `expiresAt` written as the NGSI-LD Null urn:ngsi-ld:null on a Subscription
/// expiring in 2099 answers 204, and it reads back active without `expiresAt` and with its `subscriptionName`
/// Clause: Table 5.2.12-1, 5.5.8, 5.8.3
#[gtest]
#[tokio::test]
async fn n_sub_upd_9004_an_ngsi_ld_null_expires_at_makes_the_subscription_perpetual() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_null_expiry_id = namespace.subscription_id("null-expiry")?;
    let expiry_probe_type = namespace.entity_type("ExpiryProbe")?;
    let subscription_null_expiry = Subscription::builder()
        .id(subscription_null_expiry_id.clone())
        .subscription_name("expiring")
        .entities([expiry_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .build()?;
    broker.subscriptions().create(&subscription_null_expiry).send().await?;
    let subscription = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_null_expiry_id)
        .send()
        .await?;
    expect_that!(subscription.expires_at(), some(eq(&"2099-01-01T00:00:00Z".parse::<DateTime>()?)));
    let fragment = SubscriptionFragment::builder().build()?;
    // The model refuses it (invalid timestamp at expiresAt), so `expiresAt` is written as text.
    let expires_at: RawJson = r#""urn:ngsi-ld:null""#.parse()?;
    let document = UncheckedDocument::try_from(fragment.render())?.set_member("expiresAt", &expires_at);
    let request = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_null_expiry_id,
            fragment: &fragment,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_null_expiry_id)
        .send()
        .await?;
    expect_that!(subscription_2.expires_at(), none());
    expect_that!(subscription_2.output().status, some(eq(SubscriptionStatus::Active)));
    expect_that!(subscription_2.name(), some(eq("expiring")));
    broker.subscriptions().delete(&subscription_null_expiry_id).send().await?;
    Ok(())
}
