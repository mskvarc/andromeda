//! An attribute created already expired reads as removed.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    eventually::retried,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Batch Entity Creation of an Entity with one Attribute whose `expiresAt` has passed answers 201, and the
/// Entity then reads without that Attribute and with its other one
/// Clause: 4.22, 5.6.7.4, 5.7.1.4
/// Recorded: 4.22 does not refuse an `expiresAt` already past and lets the expired attribute "be automatically
/// removed" by a periodic clean-up whose lag the spec leaves open; Andromeda accepts it and removes the attribute,
/// and the file waits up to 60 seconds for that
#[gtest]
#[tokio::test]
async fn o_bat_cre_0006_an_attribute_created_already_expired_reads_as_removed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let entities = [Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(80).expires_at("2020-01-01T00:00:00Z".parse()?))
        .property("fuel", 55)
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&vehicle_id)
            .system_attributes(SystemAttributes::Include)
            .send()
            .await?;
        let entity = retrieved.body();
        verify_that!(entity, lacks_attribute("speed"))?;
        verify_that!(entity, has_attribute("fuel", any_attribute().value(55)))?;
        Ok(())
    })
    .await?;
    Ok(())
}
