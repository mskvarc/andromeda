//! A query without type attrs q or geo query is too wide.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, model::ErrorType};

/// Pins: Query Context Source Registrations on broker A with only limit, and with only an entity id, answers 400
/// `BadRequestData`
/// Clause: 5.10.2.4
#[gtest]
#[tokio::test]
async fn n_csr_qry_7603_a_query_without_type_attrs_q_or_geo_query_is_too_wide() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::any()), Method::GET, "csourceRegistrations?limit=5")?;
    let answer = broker.with_accept(Accept::any()).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::any()),
        Method::GET,
        &format!("csourceRegistrations?id=urn%3Angsi-ld%3AVehicle%3A{namespace}-1"),
    )?;
    let answer_2 = broker.with_accept(Accept::any()).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
