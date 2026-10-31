//! A request over a map with another filter is an invalid request.

use andromeda_test_suite::{broker, header_matcher::present, namespace::Namespace, outcome::Done, problem::problem, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, ErrorType},
    query::q,
};

/// Pins: Query Entities by type naming an `EntityMap` built without q and adding q speed>99 answers 400
/// `InvalidRequest`
/// Clause: 5.5.14, 6.3.20
/// Recorded: 5.5.14 requires a request over an `EntityMap` to repeat the parameters that created it and names no
/// error for one that does not; Andromeda answers `InvalidRequest` as 6.3.20 does for incompatible parameters
#[gtest]
#[tokio::test]
async fn o_map_qen_6403_a_request_over_a_map_with_another_filter_is_an_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.entity_map(), present());
    let map = page.entity_map().valid().or_fail()?;
    let entity_map_request_2 = EntityMapRequest::reuse(map.clone());
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(q::attr("speed").gt(99))
        .entity_map(&entity_map_request_2)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
