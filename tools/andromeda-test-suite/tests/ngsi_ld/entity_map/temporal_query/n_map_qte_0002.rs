//! A posted query with a temporal query answers 201.

use andromeda_test_suite::{broker, header_matcher::present, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, NgsiLdMediaType, Property},
    query::temporal_query,
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities through POST `/temporal/entityMaps` with a
/// Query carrying entities and `temporalQ` answers 201 with an `EntityMap` mapping the Entity
/// Clause: 5.14.5.4, 6.35.3.2, 5.2.23
#[gtest]
#[tokio::test]
async fn n_map_qte_0002_a_posted_query_with_a_temporal_query_answers_201() -> Result<()> {
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
        .create_temporal(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
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
