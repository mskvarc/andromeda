//! A partial map returns only its entities.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, NgsiLdMediaType, Value},
};

/// Pins: Query Entities under an `EntityMap` of two of three Entities answers those two when it names all three ids
/// and when it names none
/// Clause: 5.5.14
#[gtest]
#[tokio::test]
async fn n_qry_pag_9010_a_partial_map_returns_only_its_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("temperature", Value::Number("30.0".parse()?))
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("temperature", Value::Number("30.0".parse()?))
            .build()?,
        Entity::builder()
            .id(building_3_id.clone())
            .entity_type(building_type.clone())
            .property("temperature", Value::Number("30.0".parse()?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&building_type)
        .ids(vec![building_id.clone(), building_2_id.clone()])
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(2)));
    let map_header = created.entity_map().valid().or_fail()?;
    let entity_map_request = EntityMapRequest::reuse(map_header.clone());
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_id.clone(), building_2_id.clone(), building_3_id.clone()])
        .entity_map(&entity_map_request)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_id)));
    expect_that!(page.items(), contains(has_id(&building_2_id)));
    let entity_map_request_2 = EntityMapRequest::reuse(map_header.clone());
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .entity_map(&entity_map_request_2)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&building_id)));
    expect_that!(page_2.items(), contains(has_id(&building_2_id)));
    let map_id = &created.body().id;
    broker.entity_maps().delete(map_id).send().await?;
    Ok(())
}
