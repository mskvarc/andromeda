//! A post query body whose entity selector has an empty id array is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, unchecked_call::exchange};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Query Entities by POST whose `entities` selector names a Vehicle type and an empty `id` array answers 400
/// `BadRequestData`, since the array names no URI and would otherwise select every Vehicle
/// Clause: 5.2.23, 5.2.33, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_ent_9015_a_post_query_body_with_an_empty_id_array_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The model refuses an empty id array, so the body goes out as written.
    let body = format!(r#"{{"type": "Query", "entities": [{{"type": "Vehicle_{namespace}", "id": []}}]}}"#);
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
