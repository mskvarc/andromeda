//! The post binding creates and reports its entity map.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{EntityMapRequest, QueryMethod},
    },
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Query Entities through POST `/entityOperations/query` with `entityMap` true answers 201 with an
/// `NGSILD-EntityMap` header naming a retrievable `EntityMap` that lists exactly the two matching Entities
/// Clause: Table 6.23.3.1-1, 6.23.1, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_ent_9002_the_post_binding_creates_and_reports_its_entity_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_mint_1_id = namespace.urn("Building", "mint-1")?;
    let building_type = namespace.entity_type("Building")?;
    let building_mint_2_id = namespace.urn("Building", "mint-2")?;
    let building_mint_1 = Entity::builder().id(building_mint_1_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building_mint_1).send().await?.done()?;
    let building_mint_2 = Entity::builder().id(building_mint_2_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building_mint_2).send().await?.done()?;
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .ids(vec![building_mint_1_id.clone(), building_mint_2_id.clone()])
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = page.entity_map().valid().or_fail()?;
    let map_id = map.id().or_fail()?;
    let map_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(&map_id)
        .send()
        .await?;
    expect_that!(&map_2.entity_map, len(eq(2)));
    expect_that!(map_2.entity_map.get(&building_mint_1_id), some(anything()));
    expect_that!(map_2.entity_map.get(&building_mint_2_id), some(anything()));
    broker.entity_maps().delete(&map_id).send().await?;
    Ok(())
}
