//! A get by type answers 201 with the map and its header.

use andromeda_test_suite::{broker, header_matcher::present, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Create `EntityMap` for Query Entities through GET `/entityMaps` with a type selector answers 201 with an
/// `EntityMap` of type `EntityMap`, an `expiresAt` and both matching Entities as keys, and an `NGSILD-EntityMap`
/// header, and the map is then retrievable
/// Clause: 5.14.4.4, 5.14.4.5, 5.2.39, 6.34.3.1
#[gtest]
#[tokio::test]
async fn n_map_qen_0001_a_get_by_type_answers_201_with_the_map_and_its_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let entities = [
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
            .build()?,
        Entity::builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(created.entity_map(), present());
    let map = created.body();
    expect_that!(map.entity_map.get(&vehicle_id), some(anything()));
    expect_that!(map.entity_map.get(&vehicle_2_id), some(anything()));
    expect_that!(&map.entity_map, len(eq(2)));
    let map_id = &created.body().id;
    let map_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(map_id)
        .send()
        .await?;
    expect_that!(&map_2.id, eq(map_id));
    Ok(())
}
