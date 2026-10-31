//! A geo query near with both min and max distance is bad request data and stores nothing.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateSubscription, UpdateSubscription},
    },
    model::{ErrorType, NgsiLdMediaType, RawJson, Representation},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Create Subscription and Update Subscription with a `geoQ` whose georel is
/// near;minDistance==500;maxDistance==10000 are refused with `BadRequestData`, nothing is stored and the stored
/// `geoQ` is kept
/// Clause: 4.10, 5.2.13, 5.8.1.4, 5.8.2, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_4004_a_geo_query_near_with_both_min_and_max_distance_is_bad_request_data_and_stores_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let spot_type = namespace.entity_type("Spot")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([spot_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    // The model refuses it (invalid geoquery at geoQ.georel), so `geoQ` is written as text.
    let geo_q: RawJson = r#"{"georel": "near;minDistance==500;maxDistance==10000", "geometry": "Point", "coordinates": [13.38893, 52.534473]}"#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("geoQ", &geo_q);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id.clone())
        .entities([spot_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    // The model refuses it (a subscription geoQ), so `geoQ` is written as text.
    let geo_q_2: RawJson = r#"{"georel": "near;maxDistance==10000", "geometry": "Point", "coordinates": [13.38893, 52.534473]}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(subscription_2.render())?.set_member("geoQ", &geo_q_2);
    let request_2 = broker.unchecked(&CreateSubscription { subscription: &subscription_2 })?.document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, answered(StatusCode::CREATED));
    let fragment = SubscriptionFragment::builder().build()?;
    // The model refuses it (invalid geoquery at geoQ.georel), so `geoQ` is written as text.
    let geo_q_3: RawJson = r#"{"georel": "near;minDistance==500;maxDistance==10000", "geometry": "Point", "coordinates": [13.38893, 52.534473]}"#.parse()?;
    let document_3 = UncheckedDocument::try_from(fragment.render())?.set_member("geoQ", &geo_q_3);
    let request_3 = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_2_id,
            fragment: &fragment,
        })?
        .document(&document_3);
    let answer_4 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await?;
    expect_that!(
        subscription_3.geo_q().map(|query| query.relation().to_string()),
        some(eq("near;maxDistance==10000"))
    );
    Ok(())
}
