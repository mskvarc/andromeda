//! An order by on a member other than id is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Query Temporal Evolution of Entities with local=true and `orderBy` naming a member other than id is
/// refused 400 `BadRequestData`
/// Clause: 5.7.4.5, 6.18.3.1
/// Recorded: the `orderBy` parameter of Query Temporal Evolution of Entities is described as the Entity member "id"
/// and the clause does not say how another member is answered; Andromeda refuses it
#[gtest]
#[tokio::test]
async fn o_fed_tmp_7416_an_order_by_on_a_member_other_than_id_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The client refuses an ordering by a member other than id, so the query is sent as written.
    let request = request_to(
        &broker,
        Method::GET,
        &format!("temporal/entities?type=Vehicle_{namespace}&local=true&orderBy=speed&timerel=before&timeAt=2030-01-01T00%3A00%3A00Z"),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
