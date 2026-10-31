//! An emptied map returns nothing.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::answered, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{EntityIdMember, EntityMapRequest},
    },
    model::{Entity, NgsiLdMediaType, Value},
    query::q,
};

/// Pins: Query Entities by type and q under an `EntityMap` of two Entities answers neither once both stop matching
/// q, which empties the `EntityMap`, and the next page answers nothing, not the matching Entity the `EntityMap`
/// never listed
/// Clause: 4.3.6.7, 5.5.14
#[gtest]
#[tokio::test]
async fn n_qry_pag_9006_an_emptied_map_returns_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_decoy_id = namespace.urn("Building", "decoy")?;
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
            .id(building_decoy_id)
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
        .q(q::attr("temperature").gt(20))
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(2)));
    let replacement = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Value::Number("1.0".parse()?))
        .build()?;
    let answer = broker
        .with_responses()
        .entities()
        .replace(&replacement)
        .id_member(EntityIdMember::Omitted)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let replacement_2 = Entity::builder()
        .id(building_2_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Value::Number("1.0".parse()?))
        .build()?;
    let answer_2 = broker
        .with_responses()
        .entities()
        .replace(&replacement_2)
        .id_member(EntityIdMember::Omitted)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    let map_header = created.entity_map().valid().or_fail()?;
    let entity_map_request = EntityMapRequest::reuse(map_header.clone());
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("temperature").gt(20))
        .limit(100)
        .entity_map(&entity_map_request)
        .send()
        .await?;
    expect_that!(page.items(), is_empty());
    let map_id = &created.body().id;
    let map_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(map_id)
        .send()
        .await?;
    expect_that!(&map_2.entity_map, len(eq(0)));
    let entity_map_request_2 = EntityMapRequest::reuse(map_header.clone());
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("temperature").gt(20))
        .limit(100)
        .entity_map(&entity_map_request_2)
        .send()
        .await?;
    expect_that!(page_2.items(), is_empty());
    broker.entity_maps().delete(map_id).send().await?;
    Ok(())
}
