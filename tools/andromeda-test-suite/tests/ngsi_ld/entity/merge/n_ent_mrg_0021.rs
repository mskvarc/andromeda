//! A merge of other attributes keeps an attribute expires at.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
};

/// Pins: Merge Entity adding an Attribute leaves the `expiresAt` of an Attribute the fragment does not name as
/// written
/// Clause: 4.22, 5.6.17.4, 6.3.11
#[gtest]
#[tokio::test]
async fn n_ent_mrg_0021_a_merge_of_other_attributes_keeps_an_attribute_expires_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(50).expires_at("2099-12-31T23:59:59Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder().property("fuel", 75).build()?;
    broker.entities().merge(&vehicle_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().expires_at("2099-12-31T23:59:59Z".parse()?)));
    expect_that!(entity, has_attribute("fuel", any_attribute().value(75)));
    Ok(())
}
