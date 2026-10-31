//! An entity created again after deletion holds only its new attributes.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property, Value},
};

/// Pins: Deleting an Entity removes every Attribute instance and sub-attribute it held, so the Entity created again
/// under the same id holds only what the new create wrote
/// Clause: 5.6.6.4, 5.6.1.4, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_del_0002_an_entity_created_again_after_deletion_holds_only_its_new_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let person_id = namespace.urn("Person", 1)?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property(
            "temperature",
            Property::builder(21).property("accuracy", Property::builder(Value::Number("0.5".parse()?)).property("method", "probe")),
        )
        .relationship("owner", person_id)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    broker.entities().delete(&building_id).send().await?.done()?;
    let building_2 = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", 18)
        .build()?;
    broker.entities().create(&building_2).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("owner"));
    expect_that!(
        entity,
        has_attribute("temperature", any_attribute().value(18).without_sub_attribute("accuracy"))
    );
    Ok(())
}
