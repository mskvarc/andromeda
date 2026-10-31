//! An entity map lifetime sets the expiry that far ahead.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType, Property},
};

/// Pins: Create `EntityMap` for Query Entities with `entityMapLifetime` PT5M answers an `expiresAt` more than 240
/// and at most 300 seconds ahead
/// Clause: 5.14.4.3, 6.4.3.2
/// Recorded: 6.4.3.2 lets the broker override the suggested `entityMapLifetime`; Andromeda honours PT5M
#[gtest]
#[tokio::test]
async fn o_map_qen_0004_an_entity_map_lifetime_sets_the_expiry_that_far_ahead() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let entities = [Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let plus_240: DateTime = (Utc::now() + TimeDelta::seconds(240)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .lifetime("PT5M".parse()?)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let plus_301: DateTime = (Utc::now() + TimeDelta::seconds(301)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let map = created.body();
    expect_that!(map.expires_at.timestamp(), gt(plus_240.timestamp()));
    expect_that!(map.expires_at.timestamp(), lt(plus_301.timestamp()));
    Ok(())
}
