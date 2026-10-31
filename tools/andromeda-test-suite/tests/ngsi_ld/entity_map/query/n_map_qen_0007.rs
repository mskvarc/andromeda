//! A posted query with an entities selector answers 201.

use andromeda_test_suite::{broker, header_matcher::present, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Create `EntityMap` for Query Entities through POST `/entityMaps` with a Query selecting the type answers
/// 201 with an `EntityMap` mapping the matching Entity
/// Clause: 5.14.4.4, 6.34.3.2
#[gtest]
#[tokio::test]
async fn n_map_qen_0007_a_posted_query_with_an_entities_selector_answers_201() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let entities = [Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .method(QueryMethod::Post)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(created.entity_map(), present());
    let map = created.body();
    expect_that!(map.entity_map.get(&vehicle_id), some(anything()));
    Ok(())
}
