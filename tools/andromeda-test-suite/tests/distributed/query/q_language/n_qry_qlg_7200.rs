//! A q ending in an operator is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType},
};

/// Pins: Query Entities by type with the q speed> answers 400 `BadRequestData`
/// Clause: 5.7.2.4, 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_7200_a_q_ending_in_an_operator_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder().id(vehicle_id).entity_type(vehicle_type).property("speed", 5).build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The model refuses this q, so the query is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::any()),
        Method::GET,
        &format!("entities?type=Vehicle_{namespace}&q=speed%3E"),
    )?;
    let answer = broker.with_accept(Accept::any()).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
