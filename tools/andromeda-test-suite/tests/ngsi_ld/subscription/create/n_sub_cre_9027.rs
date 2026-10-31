//! An accept outside the notification media types is bad request data and geojson only for entity subscriptions.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription with notification.endpoint.accept application/xml answers 400 `BadRequestData` and stores nothing, while application/json, application/ld+json and application/geo+json are created and read back; Create Context Source Registration Subscription refuses application/xml and application/geo+json the same way and creates application/json and application/ld+json
/// Clause: Table 5.2.15-1, 5.8.1.4, 5.11.2.4, 6.3.8, 6.3.9
#[gtest]
#[tokio::test]
async fn n_sub_cre_9027_an_accept_outside_the_notification_media_types_is_bad_request_data_and_geojson_only_for_entity_subscriptions() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let accept_probe_type = namespace.entity_type("AcceptProbe")?;
    let subscription_entity_xml_id = namespace.subscription_id("entity-xml")?;
    let subscription_entity_xml = Subscription::builder()
        .id(subscription_entity_xml_id.clone())
        .entities([accept_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (application/xml is not a notification media type), so `notification` is written as text.
    let notification: RawJson = format!(r#"{{"endpoint": {{"uri": "{unreachable}/notify", "accept": "application/xml"}}}}"#).parse()?;
    let document = UncheckedDocument::try_from(subscription_entity_xml.render())?.set_member("notification", &notification);
    let request = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_entity_xml,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_entity_xml_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    for (index, media_type) in [NgsiLdMediaType::Json, NgsiLdMediaType::JsonLd, NgsiLdMediaType::GeoJson]
        .into_iter()
        .enumerate()
    {
        let subscription_id = namespace.subscription_id(format!("entity-{index}"))?;
        let subscription = Subscription::builder()
            .id(subscription_id.clone())
            .entities([accept_probe_type.as_str()])
            .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(media_type).build()?).build()?)
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
        let stored = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await?;
        expect_that!(stored.notification().endpoint().accept(), some(eq(media_type)));
        broker.subscriptions().delete(&subscription_id).send().await?;
    }
    let subscription_registration_xml_id = namespace.subscription_id("registration-xml")?;
    let subscription_registration_xml = Subscription::registration_builder()
        .id(subscription_registration_xml_id.clone())
        .entities([accept_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (application/xml is not a notification media type), so `notification` is written as text.
    let notification_2: RawJson = format!(r#"{{"endpoint": {{"uri": "{unreachable}/notify", "accept": "application/xml"}}}}"#).parse()?;
    let document_2 = UncheckedDocument::try_from(subscription_registration_xml.render())?.set_member("notification", &notification_2);
    let request_2 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_registration_xml,
        })?
        .document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_registration_xml_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let subscription_registration_geo_id = namespace.subscription_id("registration-geo")?;
    let subscription_registration_geo = Subscription::registration_builder()
        .id(subscription_registration_geo_id.clone())
        .entities([accept_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::GeoJson).build()?).build()?)
        .build()?;
    let answer_5 = broker
        .with_responses()
        .registration_subscriptions()
        .create(&subscription_registration_geo)
        .send()
        .await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_registration_geo_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    for (index, media_type) in [NgsiLdMediaType::Json, NgsiLdMediaType::JsonLd].into_iter().enumerate() {
        let subscription_id = namespace.subscription_id(format!("registration-{index}"))?;
        let subscription = Subscription::registration_builder()
            .id(subscription_id.clone())
            .entities([accept_probe_type.as_str()])
            .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(media_type).build()?).build()?)
            .build()?;
        broker.registration_subscriptions().create(&subscription).send().await?;
        let stored = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .registration_subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await?;
        expect_that!(stored.notification().endpoint().accept(), some(eq(media_type)));
        broker.registration_subscriptions().delete(&subscription_id).send().await?;
    }
    Ok(())
}
