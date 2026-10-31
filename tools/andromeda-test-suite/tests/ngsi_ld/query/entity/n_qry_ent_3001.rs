//! Geometry property without a geojson accept is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{
        AttributeName,
        Entity,
        ErrorType,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
};

/// Pins: Query Entities naming `geometryProperty` is refused with `BadRequestData` under every Accept other than
/// application/geo+json, whatever the selector and the representation options, and is answered under
/// application/geo+json
/// Clause: 5.7.2.4, 6.3.15, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_ent_3001_geometry_property_without_a_geojson_accept_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let entities = [
        Entity::builder()
            .id(building_id)
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(2.2945, 48.8584)?))
            .build()?,
        Entity::builder()
            .id(building_2_id)
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // 5.7.2.4: "If geometryProperty parameter is present and the Accept Header is not set to application/geo+json, then an error of type BadRequestData shall be raised"
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Building_{namespace}&geometryProperty=location"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("entities?type=Building_{namespace}&geometryProperty=location"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // the representation options do not lift the refusal
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Building_{namespace}&geometryProperty=location&options=keyValues"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_4 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Building_{namespace}&geometryProperty=location&options=concise"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // the selector does not either: an id list, an id pattern, a q and a geo-query
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_5 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Building_{namespace}&id=urn%3Angsi-ld%3ABuilding%3A{namespace}-1&geometryProperty=location"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_6 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Building_{namespace}&q=location&geometryProperty=location"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_6 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_6).await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_7 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("entities?type=Building_{namespace}&georel=near%3BmaxDistance%3D%3D1000000&geometry=Point&coordinates=%5B2.2945%2C48.8584%5D&geometryProperty=location"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_7 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_7).await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // the same parameter under the GeoJSON Accept is answered
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .query_geo_json()
        .geometry_property(&"location".parse::<AttributeName>()?)
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    Ok(())
}
