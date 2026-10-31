//! A mapped entity deleted mid sequence leaves the map.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, NgsiLdMediaType, Value},
};

/// Pins: Query Entities by type under an `EntityMap` of four Entities, after the first page of two, answers a later
/// page without the Entity deleted in between, and the `EntityMap` no longer lists it
/// Clause: 5.5.9.3, 5.5.14, 4.3.6.7
#[gtest]
#[tokio::test]
async fn n_qry_pag_9004_a_mapped_entity_deleted_mid_sequence_leaves_the_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let building_4_id = namespace.urn("Building", 4)?;
    let entities = [
        Entity::builder()
            .id(building_id)
            .entity_type(building_type.clone())
            .property("temperature", Value::Number("30.0".parse()?))
            .build()?,
        Entity::builder()
            .id(building_2_id)
            .entity_type(building_type.clone())
            .property("temperature", Value::Number("30.0".parse()?))
            .build()?,
        Entity::builder()
            .id(building_3_id)
            .entity_type(building_type.clone())
            .property("temperature", Value::Number("30.0".parse()?))
            .build()?,
        Entity::builder()
            .id(building_4_id)
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
    expect_that!(&map.entity_map, len(eq(4)));
    let map_header = created.entity_map().valid().or_fail()?;
    let entity_map_request = EntityMapRequest::reuse(map_header.clone());
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .limit(2)
        .offset(0)
        .entity_map(&entity_map_request)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    // The third Entity of the sequence is one the first page did not offer, so it is checked again only by the pages read after it stops fitting.
    let entity_map_request_2 = EntityMapRequest::reuse(map_header.clone());
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .limit(1)
        .offset(2)
        .entity_map(&entity_map_request_2)
        .send()
        .await?;
    let victim_id = page_2.items().first().or_fail()?.id.clone();
    expect_that!(page_2.items(), len(eq(1)));
    broker.entities().delete(&victim_id).send().await?.done()?;
    let entity_map_request_3 = EntityMapRequest::reuse(map_header.clone());
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .limit(100)
        .offset(0)
        .entity_map(&entity_map_request_3)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(3)));
    expect_that!(page_3.items(), not(contains(has_id(&victim_id))));
    let map_id = &created.body().id;
    let map_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(map_id)
        .send()
        .await?;
    expect_that!(&map_2.entity_map, len(eq(3)));
    expect_that!(map_2.entity_map.get(&victim_id), none());
    broker.entity_maps().delete(map_id).send().await?;
    Ok(())
}
