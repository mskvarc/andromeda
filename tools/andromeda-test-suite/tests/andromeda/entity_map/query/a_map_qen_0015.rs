//! An entity map creation with a cursor is invalid request.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType},
};

/// Pins: Create `EntityMap` with a type, local true and an empty cursor answers 400 `InvalidRequest`, an
/// `EntityMap` creation offering no page pointer
/// Clause: 5.5.14, 6.3.20
#[gtest]
#[tokio::test]
async fn a_map_qen_0015_an_entity_map_creation_with_a_cursor_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_0000_id = namespace.urn("Building", "0000")?;
    let building_type = namespace.entity_type("Building")?;
    let building_0000 = Entity::builder()
        .id(building_0000_id)
        .entity_type(building_type)
        .property("name", "b0")
        .build()?;
    broker.entities().create(&building_0000).send().await?.done()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::any()),
        Method::GET,
        &format!("entityMaps?type=Building_{namespace}&local=true&cursor="),
    )?;
    let answer = broker.with_accept(Accept::any()).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
