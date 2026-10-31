//! A replacement with a new expires at reads back the new instant.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{DateTime, Entity, NgsiLdMediaType},
};

/// Pins: Replace Entity with a replacement carrying a different Entity-level `expiresAt` makes that instant the
/// Entity's `expiresAt`
/// Clause: 4.22, 4.3.6.8, 5.6.18.4, 6.3.11
#[gtest]
#[tokio::test]
async fn n_ent_rpl_0008_a_replacement_with_a_new_expires_at_reads_back_the_new_instant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_replacement = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .expires_at("2098-06-01T00:00:00Z".parse()?)
        .build()?;
    broker.entities().replace(&vehicle_replacement).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&vehicle_id));
    expect_that!(entity.expires_at, some(eq("2098-06-01T00:00:00Z".parse::<DateTime>()?)));
    Ok(())
}
