//! A query with a syntactically invalid q creates no entity map and is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, model::ErrorType};

/// Pins: Create `EntityMap` for Query Entities through GET with q speed> answers 400 `BadRequestData`
/// Clause: 5.14.4.4, 5.7.2.4, 4.9
#[gtest]
#[tokio::test]
async fn n_map_qen_7200_a_query_with_a_syntactically_invalid_q_creates_no_entity_map_and_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::any()),
        Method::GET,
        &format!("entityMaps?type=Vehicle_{namespace}&q=speed%3E"),
    )?;
    let answer = broker.with_accept(Accept::any()).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
