//! Geometry property naming no held geo property without a geojson accept is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Retrieve Entity refuses `geometryProperty` without the `GeoJSON` Accept before it looks at the entity: the
/// name need not be a `GeoProperty` the entity holds, and an entity without any `GeoProperty` is refused the same
/// way
/// Clause: 5.7.1.4, 6.3.15, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_3002_geometry_property_naming_no_held_geo_property_without_a_geojson_accept_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("name", "Louvre")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    // the entity holds no GeoProperty and no attribute called observationPoint: the Accept is what the rule judges
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities/{building_id}?geometryProperty=observationPoint"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // a Property that is no GeoProperty
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities/{building_id}?geometryProperty=name"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // a full IRI spelling of the name
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("entities/{building_id}?geometryProperty=https%3A%2F%2Furi.etsi.org%2Fngsi-ld%2Flocation"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
