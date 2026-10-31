//! Entity map creation refuses join level without a retrieving join.

use andromeda_test_suite::{broker, header_matcher::present, namespace::Namespace, outcome::Done, problem::problem, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::Join,
};

/// Pins: Query Entities with `entityMap=true` and `joinLevel=2` without a retrieving join answers 400
/// `InvalidRequest` and creates no map, while join=flat with `joinLevel=2` answers 201 with an `NGSILD-EntityMap`
/// header
/// Clause: 6.3.20, Table 6.4.3.2-1, 5.14.4.4
#[gtest]
#[tokio::test]
async fn n_map_qen_9008_entity_map_creation_refuses_join_level_without_a_retrieving_join() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let entity_map_request = EntityMapRequest::returned();
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .join_level(2.try_into()?)
        .entity_map(&entity_map_request)
        .send()
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    let entity_map_request_2 = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .join(Join::Flat)
        .join_level(2.try_into()?)
        .entity_map(&entity_map_request_2)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.entity_map(), present());
    Ok(())
}
