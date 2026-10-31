//! Replacing an attribute without its sub attribute removes it and everything nested under it.

use andromeda_test_suite::{attribute_matcher::property, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, NgsiLdMediaType, Property, Value},
};

/// Pins: Replace Attribute with an Attribute that omits a sub-attribute answers 204, and the sub-attribute is gone
/// together with the sub-attribute nested under it, while the other Attributes stay
/// Clause: 5.6.19.4, 4.5.2, 6.7.3.3
#[gtest]
#[tokio::test]
async fn n_att_rpl_0013_replacing_an_attribute_without_its_sub_attribute_removes_it_and_everything_nested_under_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property(
            "temperature",
            Property::builder(21).property("accuracy", Property::builder(Value::Number("0.5".parse()?)).property("method", "calibrated")),
        )
        .property("humidity", 40)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let attribute = Attribute::from(Property::builder(22).build()?);
    broker
        .entities()
        .replace_attribute(&building_id, &"temperature".parse::<AttributeName>()?, &attribute)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("temperature", property().value(22).without_sub_attribute("accuracy").sub_attribute_count(0))
    );
    expect_that!(entity, has_attribute("humidity", property().value(40)));
    Ok(())
}
