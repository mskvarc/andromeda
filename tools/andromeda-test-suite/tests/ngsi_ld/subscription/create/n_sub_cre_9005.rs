//! A geoq off the axes is refused on both kinds of subscription.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription and Create Context Source Registration Subscription with a `geoQ` near a Point at
/// latitude 100 each answer 400 `BadRequestData` naming the refused value
/// Clause: 5.8.1.4, 5.11.2.4, Table 5.2.13-1, 4.10
#[gtest]
#[tokio::test]
async fn n_sub_cre_9005_a_geoq_off_the_axes_is_refused_on_both_kinds_of_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_entities_id = namespace.subscription_id("entities")?;
    let range_probe_type = namespace.entity_type("RangeProbe")?;
    let subscription_registrations_id = namespace.subscription_id("registrations")?;
    let subscription_entities = Subscription::builder()
        .id(subscription_entities_id)
        .entities([range_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a subscription geoQ), so `geoQ` is written as text.
    let geo_q: RawJson = r#"{"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": [-1, 100]}"#.parse()?;
    let document = UncheckedDocument::try_from(subscription_entities.render())?.set_member("geoQ", &geo_q);
    let request = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_entities,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("100")
    );
    let subscription_registrations = Subscription::registration_builder()
        .id(subscription_registrations_id)
        .entities([range_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a subscription geoQ), so `geoQ` is written as text.
    let geo_q_2: RawJson = r#"{"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": [-1, 100]}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(subscription_registrations.render())?.set_member("geoQ", &geo_q_2);
    let request_2 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_registrations,
        })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("100")
    );
    Ok(())
}
