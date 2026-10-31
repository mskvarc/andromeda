//! A created map maps local entities to none and reads back.

use andromeda_test_suite::{broker, namespace::Namespace, nth::nth, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    registration::EntitySource,
};

/// Pins: an `EntityMap` created by a query maps each local Entity to `["@none"]`, has empty `linkedMaps` and an
/// `expiresAt`, and Retrieve `EntityMap` answers 200 with its id and type
/// Clause: 5.14.1.4, 5.2.39
#[gtest]
#[tokio::test]
async fn n_map_ret_0001_a_created_map_maps_local_entities_to_none_and_reads_back() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let entities = [Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?];
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
    let map = created.body();
    expect_that!(map.entity_map.get(&vehicle_id), some(len(eq(1))));
    expect_that!(map.entity_map.get(&vehicle_id), some(nth(0, eq(&EntitySource::Local))));
    expect_that!(&map.linked_maps, len(eq(0)));
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
