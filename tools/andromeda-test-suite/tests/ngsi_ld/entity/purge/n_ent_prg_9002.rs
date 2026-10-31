//! A purge under an emptied map deletes nothing.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, NgsiLdEntityMap},
    model::{Entity, NgsiLdMediaType, Value},
};

/// Pins: Purge Entities by type and three ids under an `EntityMap` created over an id no Entity has answers 204
/// naming that `EntityMap` and deletes none of them
/// Clause: 5.5.14, 6.4.3.3
#[gtest]
#[tokio::test]
async fn n_ent_prg_9002_a_purge_under_an_emptied_map_deletes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let building_absent_id = namespace.urn("Building", "absent")?;
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
        .ids(vec![building_absent_id])
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(0)));
    let map_header = created.entity_map().valid().or_fail()?;
    let received = broker
        .with_responses()
        .entities()
        .purge()
        .entity_type(&building_type)
        .ids(vec![building_id.clone(), building_2_id.clone(), building_3_id.clone()])
        .entity_map(map_header)
        .send()
        .await?;
    received.outcome.done()?;
    expect_that!(received.head.typed::<NgsiLdEntityMap>().valid(), some(eq(map_header)));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), contains(has_id(&building_id)));
    expect_that!(page.items(), contains(has_id(&building_2_id)));
    expect_that!(page.items(), contains(has_id(&building_3_id)));
    let map_id = &created.body().id;
    broker.entity_maps().delete(map_id).send().await?;
    Ok(())
}
