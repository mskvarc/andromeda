//! An attribute the entity did not carry is appended.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType},
};

/// Pins: Update Attributes naming an attribute the Entity carries and one it does not answers 204, the first
/// replaced and the second appended
/// Clause: 5.6.2.4, 5.6.2.5
#[gtest]
#[tokio::test]
async fn n_att_upd_9003_an_attribute_the_entity_did_not_carry_is_appended() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_update_id = namespace.urn("Vehicle", "update")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_update = Entity::builder()
        .id(vehicle_update_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 10)
        .build()?;
    broker.entities().create(&vehicle_update).send().await?.done()?;
    let fragment = EntityFragment::builder().property("speed", 55).property("brandName", "Mercedes").build()?;
    broker.entities().update_attributes(&vehicle_update_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_update_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(55)));
    expect_that!(entity, has_attribute("brandName", any_attribute().value("Mercedes")));
    Ok(())
}
