//! Drop mode is held to the map.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    received_status::ExpectedStatus,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, NgsiLdEntityMap},
    model::{AttributeName, Entity, NgsiLdMediaType, Value},
};

/// Pins: Purge Entities with drop temperature by type and three ids under an `EntityMap` of two of them answers 204
/// naming the `EntityMap`, deletes no Entity, and removes temperature from those two only
/// Clause: 5.6.21.4, 6.4.3.3, 5.5.14
#[gtest]
#[tokio::test]
async fn n_ent_prg_9006_drop_mode_is_held_to_the_map() -> Result<()> {
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
            .property("name", "named")
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("temperature", Value::Number("30.0".parse()?))
            .property("name", "named")
            .build()?,
        Entity::builder()
            .id(building_3_id.clone())
            .entity_type(building_type.clone())
            .property("temperature", Value::Number("30.0".parse()?))
            .property("name", "named")
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
    let received = broker
        .with_responses()
        .entities()
        .purge()
        .entity_type(&building_type)
        .ids(vec![building_id.clone(), building_2_id.clone(), building_3_id.clone()])
        .drop(vec!["temperature".parse::<AttributeName>()?])
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
    expect_that!(page.items(), contains(all!(has_id(&building_id), lacks_attribute("temperature"))));
    expect_that!(page.items(), contains(all!(has_id(&building_2_id), lacks_attribute("temperature"))));
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&building_3_id),
            has_attribute("temperature", property().value(Value::Number("30".parse()?)))
        ))
    );
    expect_that!(page.items(), each(has_attribute("name", property().value("named"))));
    let map_id = &created.body().id;
    broker.entity_maps().delete(map_id).send().await?;
    Ok(())
}
