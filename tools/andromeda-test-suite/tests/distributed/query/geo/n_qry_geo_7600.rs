//! A georel without geometry is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, model::ErrorType};

/// Pins: Query Entities by type with georel near;maxDistance==1000 and no geometry or coordinates answers 400
/// `BadRequestData` on broker A
/// Clause: 5.7.2.4, 4.10
#[gtest]
#[tokio::test]
async fn n_qry_geo_7600_a_georel_without_geometry_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::any()),
        Method::GET,
        &format!("entities?type=Vehicle_{namespace}&georel=near%3BmaxDistance%3D%3D1000"),
    )?;
    let answer = broker.with_accept(Accept::any()).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::any()),
        Method::GET,
        &format!("entities?type=Vehicle_{namespace}&georel=near%3BmaxDistance%3D%3D1000"),
    )?;
    let answer_2 = broker.with_accept(Accept::any()).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
