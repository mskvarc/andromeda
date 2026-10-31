//! A narrower follow up keeps the other entities.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, NgsiLdMediaType, Value},
    query::q,
};

/// Pins: Query Entities by type, q and the id of one of the three Entities an `EntityMap` lists answers that Entity
/// alone, and the `EntityMap` keeps all three, the other two never having been checked
/// Clause: 5.5.14, 4.3.6.7
#[gtest]
#[tokio::test]
async fn n_qry_pag_9007_a_narrower_follow_up_keeps_the_other_entities() -> Result<()> {
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
            .id(building_2_id)
            .entity_type(building_type.clone())
            .property("temperature", Value::Number("30.0".parse()?))
            .build()?,
        Entity::builder()
            .id(building_3_id)
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
    expect_that!(&map.entity_map, len(eq(3)));
    let map_header = created.entity_map().valid().or_fail()?;
    let entity_map_request = EntityMapRequest::reuse(map_header.clone());
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .q(q::attr("temperature").gt(20))
        .ids(vec![building_id.clone()])
        .entity_map(&entity_map_request)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_id)));
    let map_id = &created.body().id;
    let map_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(map_id)
        .send()
        .await?;
    expect_that!(&map_2.entity_map, len(eq(3)));
    broker.entity_maps().delete(map_id).send().await?;
    Ok(())
}
