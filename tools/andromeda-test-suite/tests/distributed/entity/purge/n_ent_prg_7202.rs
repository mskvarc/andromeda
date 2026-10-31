//! A purge whose q ends in an operator is bad request data and purges nothing.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    raw_call::target,
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType},
    query::Locality,
};

/// Pins: Purge Entities by type with the q `speed>` answers 400 `BadRequestData` and the Entity is not purged
/// Clause: 5.6.21.4, 4.9, 6.4.3.3
#[gtest]
#[tokio::test]
async fn n_ent_prg_7202_a_purge_whose_q_ends_in_an_operator_is_bad_request_data_and_purges_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The query model refuses a `q` that ends in an operator, so the request is sent as written.
    let request = request_to(
        &broker,
        Method::DELETE,
        &target("entities", &[("type", vehicle_type.as_str()), ("q", "speed>")]),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_attribute("speed", any_attribute().value(5)));
    Ok(())
}
