//! An attribute expires at stands beside the entity expires at.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{DateTime, Entity, NgsiLdMediaType, Property},
};

/// Pins: an Entity-level `expiresAt` and an earlier `expiresAt` on one of its Attributes read back each as written,
/// neither taking the other's instant
/// Clause: 4.22, 4.3.6.8, 6.3.11, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_0040_an_attribute_expires_at_stands_beside_the_entity_expires_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .property("speed", Property::builder(60).expires_at("2098-06-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.expires_at, some(eq("2099-01-01T00:00:00Z".parse::<DateTime>()?)));
    expect_that!(entity, has_attribute("speed", any_attribute().expires_at("2098-06-01T00:00:00Z".parse()?)));
    Ok(())
}
