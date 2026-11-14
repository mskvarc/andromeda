//! A `GeometryCollection` reference geometry is refused in a query and a subscription.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Entities with geometry=GeometryCollection, and Create Subscription with a `geoQ` whose geometry is
/// `GeometryCollection`, each answer 400 `BadRequestData`; the reference geometry may be any `GeoJSON` geometry
/// type except `GeometryCollection`
/// Clause: 4.10, 5.2.13, 5.7.2.4, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_qry_geo_9093_a_geometry_collection_reference_geometry_is_refused_in_a_query_and_a_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let probe_type = namespace.entity_type("CollectionProbe")?;
    let subscription_id = namespace.subscription_id("collection")?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=CollectionProbe_{namespace}&georel=intersects&geometry=GeometryCollection&coordinates=%5B13.4%2C52.5%5D"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription = Subscription::builder()
        .id(subscription_id)
        .entities([probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a GeometryCollection reference at geoQ), so `geoQ` is written as text.
    let geo_q: RawJson = r#"{"georel": "intersects", "geometry": "GeometryCollection", "coordinates": [13.4, 52.5]}"#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("geoQ", &geo_q);
    let subscription_request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let subscription_answer = broker.send_unchecked(subscription_request).await;
    expect_that!(&subscription_answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
