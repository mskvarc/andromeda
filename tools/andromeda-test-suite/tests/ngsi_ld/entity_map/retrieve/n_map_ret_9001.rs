//! An entity map reads back as it was created.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Value},
    registration::EntitySource,
};

/// Pins: Retrieve `EntityMap` answers the id, type, `expiresAt`, `entityMap` and `linkedMaps` the Create
/// `EntityMap` for Query Entities that created it answered
/// Clause: 5.2.39, 5.14.1.4, 6.3.19
#[gtest]
#[tokio::test]
async fn n_map_ret_9001_an_entity_map_reads_back_as_it_was_created() -> Result<()> {
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
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(2)));
    expect_that!(&map.linked_maps, len(eq(0)));
    let map_id = &created.body().id;
    let map_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(map_id)
        .send()
        .await?;
    expect_that!(&map_2.id, eq(map_id));
    expect_that!(&map_2.expires_at, eq(&created.body().expires_at));
    expect_that!(&map_2.entity_map, len(eq(2)));
    expect_that!(map_2.entity_map.get(&building_id), some(contains(eq(&EntitySource::Local))));
    expect_that!(map_2.entity_map.get(&building_2_id), some(contains(eq(&EntitySource::Local))));
    expect_that!(&map_2.linked_maps, len(eq(0)));
    broker.entity_maps().delete(map_id).send().await?;
    Ok(())
}
