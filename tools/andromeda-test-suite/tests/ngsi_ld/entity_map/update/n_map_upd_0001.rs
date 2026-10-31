//! A future expires at answers 204 and the map stands.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType, Property},
};

/// Pins: Update `EntityMap` with an `expiresAt` two hours ahead answers 204, and the `EntityMap` is then
/// retrievable
/// Clause: 5.14.2.4, 6.32.3.2
#[gtest]
#[tokio::test]
async fn n_map_upd_0001_a_future_expires_at_answers_204_and_the_map_stands() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let plus_2h: DateTime = (Utc::now() + TimeDelta::hours(2)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let entities = [Entity::builder()
        .id(vehicle_id)
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
    let map_id = &created.body().id;
    broker.entity_maps().update(map_id, &plus_2h).send().await?;
    let map = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(map_id)
        .send()
        .await?;
    expect_that!(&map.id, eq(map_id));
    Ok(())
}
