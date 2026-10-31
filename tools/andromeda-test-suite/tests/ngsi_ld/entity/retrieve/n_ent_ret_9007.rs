//! A retrieval under a map sheds nothing.

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

/// Pins: Retrieve Entity under an `EntityMap` of two Entities, one of which has stopped matching the q the map was
/// created with, answers 200 and leaves both Entities in the `EntityMap`, a retrieval applying no query filter
/// Clause: 4.3.6.7, 5.5.14, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_9007_a_retrieval_under_a_map_sheds_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
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
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&building_type)
        .q(q::attr("temperature").gt(20))
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(2)));
    let replacement = Entity::builder()
        .id(building_2_id.clone())
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
    let map_header = created.entity_map().valid().or_fail()?;
    let entity_map_request = EntityMapRequest::reuse(map_header.clone());
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .entity_map(&entity_map_request)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    let map_id = &created.body().id;
    let map_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(map_id)
        .send()
        .await?;
    expect_that!(&map_2.entity_map, len(eq(2)));
    expect_that!(map_2.entity_map.get(&building_id), some(anything()));
    expect_that!(map_2.entity_map.get(&building_2_id), some(anything()));
    broker.entity_maps().delete(map_id).send().await?;
    Ok(())
}
