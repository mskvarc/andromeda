//! A merge moves an attribute expires at.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
};

/// Pins: Merge Entity naming an Attribute with a new `expiresAt` makes that instant the Attribute's `expiresAt`
/// Clause: 4.22, 5.6.17.4, 6.3.11
#[gtest]
#[tokio::test]
async fn n_ent_mrg_0022_a_merge_moves_an_attribute_expires_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(60).expires_at("2099-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("speed", Property::builder(60).expires_at("2099-06-30T00:00:00Z".parse()?))
        .build()?;
    broker.entities().merge(&vehicle_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().expires_at("2099-06-30T00:00:00Z".parse()?)));
    Ok(())
}
