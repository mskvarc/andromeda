//! An accept outside the notification media types is bad request data and keeps the stored one.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::UpdateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription naming notification.endpoint.accept application/xml answers 400 `BadRequestData` and keeps the stored accept; Update Context Source Registration Subscription refuses application/xml and application/geo+json the same way
/// Clause: Table 5.2.15-1, 5.8.2.4, 5.11.3.4, 6.3.9
#[gtest]
#[tokio::test]
async fn n_sub_upd_9011_an_accept_outside_the_notification_media_types_is_bad_request_data_and_keeps_the_stored_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_entity_id = namespace.subscription_id("entity")?;
    let accept_probe_type = namespace.entity_type("AcceptProbe")?;
    let subscription_registration_id = namespace.subscription_id("registration")?;
    let subscription_entity = Subscription::builder()
        .id(subscription_entity_id.clone())
        .entities([accept_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::JsonLd).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_entity).send().await?;
    let fragment = SubscriptionFragment::builder()
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (application/xml is not a notification media type), so `notification` is written as text.
    let notification: RawJson = format!(r#"{{"endpoint": {{"uri": "{unreachable}/notify", "accept": "application/xml"}}}}"#).parse()?;
    let document = UncheckedDocument::try_from(fragment.render())?.set_member("notification", &notification);
    let request = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_entity_id,
            fragment: &fragment,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_entity_id)
        .send()
        .await?;
    expect_that!(stored.notification().endpoint().accept(), some(eq(NgsiLdMediaType::JsonLd)));
    let subscription_registration = Subscription::registration_builder()
        .id(subscription_registration_id.clone())
        .entities([accept_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::JsonLd).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription_registration).send().await?;
    let fragment_registration = SubscriptionFragment::registration_builder()
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (application/xml is not a notification media type), so `notification` is written as text.
    let notification_2: RawJson = format!(r#"{{"endpoint": {{"uri": "{unreachable}/notify", "accept": "application/xml"}}}}"#).parse()?;
    let document_2 = UncheckedDocument::try_from(fragment_registration.render())?.set_member("notification", &notification_2);
    let request_2 = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_registration_id,
            fragment: &fragment_registration,
        })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let stored_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_registration_id)
        .send()
        .await?;
    expect_that!(stored_2.notification().endpoint().accept(), some(eq(NgsiLdMediaType::JsonLd)));
    let fragment_2 = SubscriptionFragment::registration_builder()
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::GeoJson).build()?).build()?)
        .build()?;
    let answer_3 = broker
        .with_responses()
        .registration_subscriptions()
        .update(&subscription_registration_id, &fragment_2)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let stored_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_registration_id)
        .send()
        .await?;
    expect_that!(stored_3.notification().endpoint().accept(), some(eq(NgsiLdMediaType::JsonLd)));
    broker.subscriptions().delete(&subscription_entity_id).send().await?;
    broker.registration_subscriptions().delete(&subscription_registration_id).send().await?;
    Ok(())
}
