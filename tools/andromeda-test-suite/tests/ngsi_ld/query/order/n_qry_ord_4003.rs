//! A distance ordering without order from is bad request data without the local parameter.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        ErrorType,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
};

/// Pins: orderBy=location;dist-asc without `orderFrom` is refused with `BadRequestData` beside a near geo-query
/// also when the request does not carry local=true
/// Clause: 5.7.2.4, 6.4.3.2, 4.23.3
#[gtest]
#[tokio::test]
async fn n_qry_ord_4003_a_distance_ordering_without_order_from_is_bad_request_data_without_the_local_parameter() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let entities = [Entity::builder()
        .id(building_id)
        .entity_type(building_type)
        .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("entities?type=Building_{namespace}&georel=near%3BmaxDistance%3D%3D50000&geometry=Point&coordinates=%5B13.3986%2C52.5547%5D&orderBy=location%3Bdist-asc"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("entities?type=Building_{namespace}&georel=near%3BmaxDistance%3D%3D50000&geometry=Point&coordinates=%5B13.3986%2C52.5547%5D&geoproperty=location&orderBy=location%3Bdist-desc"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
