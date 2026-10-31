//! An incomplete geoq is refused on create and update.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateSubscription, UpdateSubscription},
    },
    model::{
        ErrorType,
        NgsiLdMediaType,
        RawJson,
        geometry::{Position, Shape},
    },
    query::geoquery,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Create Subscription with a `geoQ` holding only georel answers 400 `BadRequestData`, a complete `geoQ` is
/// created, and Update Subscription with a `geoQ` lacking coordinates answers 400 `BadRequestData` and leaves the
/// stored `geoQ` as it was
/// Clause: 5.8.1.4, 5.8.2.4, Table 5.2.13-1, 5.5.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_9006_an_incomplete_geoq_is_refused_on_create_and_update() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let probe_type = namespace.entity_type("RangeProbe")?;
    let notification = NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?;
    let incomplete = Subscription::builder()
        .id(subscription_id.clone())
        .entities([probe_type.as_str()])
        .notification(notification.clone())
        .build()?;
    // The model has no geoQ without geometry or coordinates, so that member is written as text.
    let georel_only: RawJson = r#"{"georel": "near;maxDistance==2000"}"#.parse()?;
    let document = UncheckedDocument::try_from(incomplete.render())?.set_member("geoQ", &georel_only);
    let request = broker.unchecked(&CreateSubscription { subscription: &incomplete })?.document(&document);
    let refused = broker.send_unchecked(request).await;
    expect_that!(
        &refused,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("geometry")
    );
    let geo_q = geoquery::near_max(2000, Shape::Point(Position::try_new(14.5, 46.05)?)).build()?;
    let complete = Subscription::builder()
        .id(subscription_id.clone())
        .entities([probe_type.as_str()])
        .geo_q(geo_q.clone())
        .notification(notification)
        .build()?;
    broker.subscriptions().create(&complete).send().await?;
    let fragment = SubscriptionFragment::builder().build()?;
    // The model has no geoQ without coordinates, so that member is written as text.
    let no_coordinates: RawJson = r#"{"georel": "within", "geometry": "Point"}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(fragment.render())?.set_member("geoQ", &no_coordinates);
    let request_2 = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_id,
            fragment: &fragment,
        })?
        .document(&document_2);
    let refused_2 = broker.send_unchecked(request_2).await;
    expect_that!(
        &refused_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("coordinates")
    );
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(stored.geo_q(), some(eq(&geo_q)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
