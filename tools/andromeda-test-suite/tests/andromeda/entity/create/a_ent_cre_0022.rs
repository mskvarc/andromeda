//! A sub attribute chain at the nesting limit reads back as written.

use andromeda_test_suite::{attribute_matcher::property, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// The levels of the chain, the Attribute itself counted.
const LEVELS: i32 = 32;

/// Pins: An Attribute whose sub-attributes nest 32 levels deep, the default nesting limit counting the Attribute
/// itself, reads back level for level as written
/// Clause: 5.6.1.4, 5.7.1.4, 4.5.2
#[gtest]
#[tokio::test]
async fn a_ent_cre_0022_a_sub_attribute_chain_at_the_nesting_limit_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let mut chain = Property::builder(LEVELS).build()?;
    for level in (1..LEVELS).rev() {
        chain = Property::builder(level).property("deeper", chain).build()?;
    }
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("deep", chain)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    // The innermost level holds no sub-attribute, so a 33rd nested member is absent.
    let mut expected = property().value(LEVELS).without_sub_attribute("deeper");
    for level in (1..LEVELS).rev() {
        expected = property().value(level).sub_attribute("deeper", expected);
    }
    expect_that!(entity, has_attribute("deep", expected));
    Ok(())
}
