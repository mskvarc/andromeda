//! A geoq reference latitude off the wgs84 axes is bad request data.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription whose `geoQ` reference Point has latitude 100 is refused with 400 `BadRequestData` and
/// creates nothing
/// Clause: 4.10, 5.8.1, 6.3.2
#[gtest]
#[tokio::test]
async fn n_sub_cre_0005_a_geoq_reference_latitude_off_the_wgs84_axes_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_off_axis_id = namespace.subscription_id("off-axis")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_off_axis = Subscription::builder()
        .id(subscription_off_axis_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a subscription geoQ), so `geoQ` is written as text.
    let geo_q: RawJson = r#"{"georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": [0, 100]}"#.parse()?;
    let document = UncheckedDocument::try_from(subscription_off_axis.render())?.set_member("geoQ", &geo_q);
    let request = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_off_axis,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::any())
        .subscriptions()
        .retrieve(&subscription_off_axis_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
