//! One matching geo property instance keeps the entity in the map.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        GeoProperty,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::geoquery,
    registration::EntitySource,
};

/// Pins: Create `EntityMap` for Query Entities with a near geoquery maps an Entity whose location holds a near
/// instance and a far one, and leaves out an Entity whose only instance is far
/// Clause: 5.14.4.4, 4.10
#[gtest]
#[tokio::test]
async fn n_map_qen_7006_one_matching_geo_property_instance_keeps_the_entity_in_the_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let entities = [
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .geo_property(
                "location",
                GeoProperty::builder(Shape::Point(Position::try_new(0.0, 0.0)?)).dataset_id(namespace.dataset_id("Dataset", "here")?),
            )
            .geo_property(
                "location",
                GeoProperty::builder(Shape::Point(Position::try_new(80.0, 80.0)?)).dataset_id(namespace.dataset_id("Dataset", "far")?),
            )
            .build()?,
        Entity::builder()
            .id(vehicle_2_id)
            .entity_type(vehicle_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(80.0, 80.0)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .geo_q(geoquery::near_max(2000, Shape::Point(Position::try_new(0.0, 0.0)?)))
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(1)));
    expect_that!(map.entity_map.get(&vehicle_id), some(contains(eq(&EntitySource::Local))));
    Ok(())
}
