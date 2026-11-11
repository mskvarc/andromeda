//! A map created through get serves a post query of its type.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, problem::problem, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{EntityMapRequest, QueryMethod},
    },
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Query Entities through POST with one `EntitySelector` of a type under an `EntityMap` that Create `EntityMap`
/// for Query Entities through GET created by that type answers the mapped Entity, and through POST with one
/// `EntitySelector` of another type answers 400 `InvalidRequest`
/// Clause: 5.5.14, 5.7.2.3, 5.2.33, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_map_qen_9012_a_map_created_through_get_serves_a_post_query_of_its_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let building_type = namespace.entity_type("Building")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let created = json
        .with_heads()
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let entity_map_request = EntityMapRequest::reuse(created.entity_map().valid().or_fail()?.clone());
    let page = json
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&vehicle_type)
        .entity_map(&entity_map_request)
        .send()
        .await?;
    expect_that!(page.items(), elements_are![has_id(&vehicle_id)]);
    let answer = json
        .with_responses()
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .entity_map(&entity_map_request)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    broker.entity_maps().delete(&created.body().id).send().await?;
    Ok(())
}
