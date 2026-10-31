//! Flat appends every target of a list relationship.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    broker,
    entity_lookup::with_id,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ListRelationship, NgsiLdMediaType},
    query::Join,
};

/// Pins: Query Entities with join=flat and `joinLevel=1` answers the matching entity followed by every target of
/// its `ListRelationship`'s `objectList`
/// Clause: 4.5.23.3, 4.5.22, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_qry_0006_flat_appends_every_target_of_a_list_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_1_id = namespace.urn("Sensor", 1)?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let sensor_3_id = namespace.urn("Sensor", 3)?;
    let building_type = namespace.entity_type("Building")?;
    let building_1_id = namespace.urn("Building", 1)?;
    let entities = [
        Entity::builder()
            .id(sensor_1_id.clone())
            .entity_type(sensor_type.clone())
            .property("name", "one")
            .build()?,
        Entity::builder()
            .id(sensor_2_id.clone())
            .entity_type(sensor_type.clone())
            .property("name", "two")
            .build()?,
        Entity::builder()
            .id(sensor_3_id.clone())
            .entity_type(sensor_type.clone())
            .property("name", "three")
            .build()?,
        Entity::builder()
            .id(building_1_id.clone())
            .entity_type(building_type.clone())
            .list_relationship(
                "hasSensors",
                ListRelationship::builder(vec![sensor_1_id.clone(), sensor_2_id.clone(), sensor_3_id.clone()]),
            )
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_1_id.clone()])
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(4)));
    expect_that!(page.items(), nth(0, has_id(&building_1_id)));
    expect_that!(page.items(), nth(0, has_attribute("hasSensors", any_attribute().list_length(3))));
    for (sensor_id, name) in [(&sensor_1_id, "one"), (&sensor_2_id, "two"), (&sensor_3_id, "three")] {
        let sensor = with_id(page.items(), sensor_id).or_fail()?;
        expect_that!(sensor, has_attribute("name", property().value(name)));
    }
    Ok(())
}
