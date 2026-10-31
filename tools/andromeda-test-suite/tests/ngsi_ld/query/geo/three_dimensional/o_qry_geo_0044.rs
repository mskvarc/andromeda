//! A reference position of four ordinates is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{
        ErrorType,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: a geo-query reference position of four ordinates is refused with `BadRequestData`, by GET and in a Query
/// body, while three ordinates in the same Query body are admitted
/// Clause: 4.10, 5.7.2.4, 6.4.3.2, 6.23.3.1
/// Recorded: RFC 7946 section 3.1.1 says implementations "SHOULD NOT extend positions beyond three elements" and
/// that parsers "MAY ignore" extra elements, and 4.7.1 requires a valid `GeoJSON` geometry without saying what a
/// fourth element is, so the specification does not require the refusal; Andromeda refuses a position of four
/// ordinates as `BadRequestData` (Table 5.5.2-1)
#[gtest]
#[tokio::test]
async fn o_qry_geo_0044_a_reference_position_of_four_ordinates_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Building_{namespace}&georel=near%3BmaxDistance%3D%3D100&geometry=Point&coordinates=%5B13.4%2C52.52%2C300.0%2C7.0%5D"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The same four-ordinate reference in a Query body.
    // The model refuses a member of this document, so it is sent as written.
    let request_2 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"type\": \"Query\",\n    \"entities\": [\n        {{\n            \"type\": \"Building_{namespace}\"\n        }}\n    ],\n    \"geoQ\": {{\n        \"georel\": \"near;maxDistance==100\",\n        \"geometry\": \"Point\",\n        \"coordinates\": [13.4, 52.52, 300.0, 7.0]\n    }}\n}}")));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // Three ordinates in the same Query body are admitted.
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .geo_q(geoquery::near_max(100, Shape::Point(Position::try_with_altitude(13.4, 52.52, 300.0)?)))
        .entity_type(&namespace.entity_type("Building")?)
        .send()
        .await?;
    Ok(())
}
