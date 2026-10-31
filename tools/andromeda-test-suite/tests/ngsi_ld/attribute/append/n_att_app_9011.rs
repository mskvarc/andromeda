//! A second default instance replaces the first.

use andromeda_test_suite::{attribute_matcher::property, attribute_member::Member, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType},
};

/// Pins: Append Attributes with temperature 2 over an Entity holding the default temperature instance 1 answers
/// 204, and the Entity holds one default temperature instance with value 2
/// Clause: 5.6.3.4, 4.5.5
#[gtest]
#[tokio::test]
async fn n_att_app_9011_a_second_default_instance_replaces_the_first() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_default_grain_id = namespace.urn("Building", "default-grain")?;
    let building_type = namespace.entity_type("Building")?;
    let building_default_grain = Entity::builder()
        .id(building_default_grain_id.clone())
        .entity_type(building_type)
        .property("temperature", 1)
        .build()?;
    broker.entities().create(&building_default_grain).send().await?.done()?;
    let fragment = EntityFragment::builder().property("temperature", 2).build()?;
    broker
        .entities()
        .append_attributes(&building_default_grain_id, &fragment)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_default_grain_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("temperature", property().value(2).without(Member::DatasetId)));
    Ok(())
}
