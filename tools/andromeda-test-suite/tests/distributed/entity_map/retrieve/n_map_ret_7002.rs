//! A created map has a uri id a date time expiry and holder lists.

use andromeda_test_suite::{broker, namespace::Namespace, nth::nth, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    registration::EntitySource,
};

/// Pins: an `EntityMap` created by a query has a URI id, type `EntityMap`, an `expiresAt` that is a `DateTime` and
/// an `entityMap` object mapping the Entity id to an array holding the @none marker for a locally held Entity, in
/// the creation answer and on Retrieve `EntityMap` alike
/// Clause: 5.2.39, 4.6.3, 5.14.1.4
#[gtest]
#[tokio::test]
async fn n_map_ret_7002_a_created_map_has_a_uri_id_a_date_time_expiry_and_holder_lists() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 20)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
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
    expect_that!(map.entity_map.get(&vehicle_id), some(anything()));
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
    expect_that!(map_2.entity_map.get(&vehicle_id), some(anything()));
    expect_that!(map_2.entity_map.get(&vehicle_id), some(nth(0, eq(&EntitySource::Local))));
    Ok(())
}
