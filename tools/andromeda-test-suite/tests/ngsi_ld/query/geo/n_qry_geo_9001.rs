//! A reference geometry off the axes is refused on every query.

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
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Query Entities with a near Point at longitude 1000 or a within Polygon with a vertex at latitude 91, Query
/// Temporal Evolution with the near Point, Query Entities ordered by distance from latitude 91, Query Context
/// Source Registrations with the near Point and the POST binding of Query Entities with a `geoQ` at latitude 91
/// each answer 400 `BadRequestData` naming the refused value
/// Clause: 4.10, 4.23, 5.5.4, 6.4.3.2, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_geo_9001_a_reference_geometry_off_the_axes_is_refused_on_every_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=RangeProbe_{namespace}&georel=near%3BmaxDistance%3D%3D10&geometry=Point&coordinates=%5B1000%2C2000%5D"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("1000")
    );
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("entities?type=RangeProbe_{namespace}&georel=within&geometry=Polygon&coordinates=%5B%5B%5B0%2C0%5D%2C%5B10%2C0%5D%2C%5B10%2C91%5D%2C%5B0%2C0%5D%5D%5D"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("91")
    );
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("temporal/entities?type=RangeProbe_{namespace}&timerel=after&timeAt=2020-01-01T00%3A00%3A00Z&georel=near%3BmaxDistance%3D%3D10&geometry=Point&coordinates=%5B1000%2C2000%5D"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("1000")
    );
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_4 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=RangeProbe_{namespace}&orderBy=location%3Bdist-asc&orderFrom=%5B0%2C91%5D"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(
        &answer_4,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("91")
    );
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_5 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("csourceRegistrations?type=RangeProbe_{namespace}&georel=near%3BmaxDistance%3D%3D10&geometry=Point&coordinates=%5B1000%2C2000%5D"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_5).await;
    expect_that!(
        &answer_5,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("1000")
    );
    // The model refuses a member of this document, so it is sent as written.
    let request_6 = request_to(&broker, Method::POST, "entityOperations/query")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"type\": \"Query\",\n    \"entities\": [\n        {{\n            \"type\": \"RangeProbe_{namespace}\"\n        }}\n    ],\n    \"geoQ\": {{\n        \"georel\": \"near;maxDistance==10\",\n        \"geometry\": \"Point\",\n        \"coordinates\": [\n            0,\n            91\n        ]\n    }}\n}}")));
    let answer_6 = broker.send_unchecked(request_6).await;
    expect_that!(
        &answer_6,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("91")
    );
    Ok(())
}
