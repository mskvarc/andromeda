//! A post query body of another type is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, unchecked_call::exchange};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Query Entities by POST with a body whose type is Subscription answers 400 `BadRequestData`
/// Clause: 5.2.23, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_ent_9006_a_post_query_body_of_another_type_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // A Query document is typed Query; the model writes no other type, so the body goes out as written.
    let body = format!(r#"{{"type": "Subscription", "entities": [{{"type": "Building_{namespace}"}}]}}"#);
    let request = exchange(
        &broker,
        Method::POST,
        "entityOperations/query",
        &[("Content-Type", "application/json")],
        Some(body.as_str()),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
