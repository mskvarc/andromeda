//! A write selecting the entity by type applies.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, EntityFragment, ErrorType, NgsiLdMediaType, Property},
    query::type_selection,
};

/// Pins: Delete Entity, Replace Entity, Append, Update, Partial Attribute update, Delete Attribute and Replace
/// Attribute with type naming the Vehicle's type, alone or among others, each answer 204 and apply
/// Clause: 4.17, Tables 6.5.3.2-1 to 6.7.3.3-1, 5.6.2.4 to 5.6.6.4, 5.6.18.4, 5.6.19.4
#[gtest]
#[tokio::test]
async fn n_ent_rpl_9006_a_write_selecting_the_entity_by_type_applies() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let other_type = namespace.entity_type("Other")?;
    let vehicle_inside_delete_entity_0_id = namespace.urn("Vehicle", "inside-delete-entity-0")?;
    let vehicle_inside_delete_entity_0 = Entity::builder()
        .id(vehicle_inside_delete_entity_0_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_inside_delete_entity_0).send().await?.done()?;
    broker
        .entities()
        .delete(&vehicle_inside_delete_entity_0_id)
        .entity_type(&vehicle_type)
        .send()
        .await?
        .done()?;
    let answer_read = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_inside_delete_entity_0_id)
        .send()
        .await;
    expect_that!(&answer_read, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let vehicle_inside_delete_entity_1_id = namespace.urn("Vehicle", "inside-delete-entity-1")?;
    let vehicle_inside_delete_entity_1 = Entity::builder()
        .id(vehicle_inside_delete_entity_1_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_inside_delete_entity_1).send().await?.done()?;
    broker
        .entities()
        .delete(&vehicle_inside_delete_entity_1_id)
        .entity_type(type_selection::entity_type(&vehicle_type).or(&other_type))
        .send()
        .await?
        .done()?;
    let answer_read = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_inside_delete_entity_1_id)
        .send()
        .await;
    expect_that!(&answer_read, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let vehicle_inside_replace_entity_0_id = namespace.urn("Vehicle", "inside-replace-entity-0")?;
    let vehicle_inside_replace_entity_0 = Entity::builder()
        .id(vehicle_inside_replace_entity_0_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_inside_replace_entity_0).send().await?.done()?;
    let replacement = Entity::builder()
        .id(vehicle_inside_replace_entity_0_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().replace(&replacement).entity_type(&vehicle_type).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_inside_replace_entity_0_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", property().value(1)));
    expect_that!(entity, lacks_attribute("plate"));
    let vehicle_inside_replace_entity_1_id = namespace.urn("Vehicle", "inside-replace-entity-1")?;
    let vehicle_inside_replace_entity_1 = Entity::builder()
        .id(vehicle_inside_replace_entity_1_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_inside_replace_entity_1).send().await?.done()?;
    let replacement = Entity::builder()
        .id(vehicle_inside_replace_entity_1_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker
        .entities()
        .replace(&replacement)
        .entity_type(type_selection::entity_type(&vehicle_type).or(&other_type))
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_inside_replace_entity_1_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", property().value(1)));
    expect_that!(entity, lacks_attribute("plate"));
    let vehicle_inside_append_attributes_0_id = namespace.urn("Vehicle", "inside-append-attributes-0")?;
    let vehicle_inside_append_attributes_0 = Entity::builder()
        .id(vehicle_inside_append_attributes_0_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_inside_append_attributes_0).send().await?.done()?;
    let fragment = EntityFragment::builder().property("color", "red").build()?;
    broker
        .entities()
        .append_attributes(&vehicle_inside_append_attributes_0_id, &fragment)
        .entity_type(&vehicle_type)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_inside_append_attributes_0_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("color", property().value("red")));
    let vehicle_inside_append_attributes_1_id = namespace.urn("Vehicle", "inside-append-attributes-1")?;
    let vehicle_inside_append_attributes_1 = Entity::builder()
        .id(vehicle_inside_append_attributes_1_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_inside_append_attributes_1).send().await?.done()?;
    let fragment = EntityFragment::builder().property("color", "red").build()?;
    broker
        .entities()
        .append_attributes(&vehicle_inside_append_attributes_1_id, &fragment)
        .entity_type(type_selection::entity_type(&vehicle_type).or(&other_type))
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_inside_append_attributes_1_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("color", property().value("red")));
    let vehicle_inside_update_attributes_0_id = namespace.urn("Vehicle", "inside-update-attributes-0")?;
    let vehicle_inside_update_attributes_0 = Entity::builder()
        .id(vehicle_inside_update_attributes_0_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_inside_update_attributes_0).send().await?.done()?;
    let fragment = EntityFragment::builder().property("speed", 70).build()?;
    broker
        .entities()
        .update_attributes(&vehicle_inside_update_attributes_0_id, &fragment)
        .entity_type(&vehicle_type)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_inside_update_attributes_0_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", property().value(70)));
    let vehicle_inside_update_attributes_1_id = namespace.urn("Vehicle", "inside-update-attributes-1")?;
    let vehicle_inside_update_attributes_1 = Entity::builder()
        .id(vehicle_inside_update_attributes_1_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_inside_update_attributes_1).send().await?.done()?;
    let fragment = EntityFragment::builder().property("speed", 70).build()?;
    broker
        .entities()
        .update_attributes(&vehicle_inside_update_attributes_1_id, &fragment)
        .entity_type(type_selection::entity_type(&vehicle_type).or(&other_type))
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_inside_update_attributes_1_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", property().value(70)));
    let vehicle_inside_partial_attribute_update_0_id = namespace.urn("Vehicle", "inside-partial-attribute-update-0")?;
    let vehicle_inside_partial_attribute_update_0 = Entity::builder()
        .id(vehicle_inside_partial_attribute_update_0_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_inside_partial_attribute_update_0).send().await?.done()?;
    let patch = Attribute::from(Property::builder(71).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&vehicle_inside_partial_attribute_update_0_id, &"speed".parse::<AttributeName>()?, patch)
        .entity_type(&vehicle_type)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_inside_partial_attribute_update_0_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", property().value(71)));
    let vehicle_inside_partial_attribute_update_1_id = namespace.urn("Vehicle", "inside-partial-attribute-update-1")?;
    let vehicle_inside_partial_attribute_update_1 = Entity::builder()
        .id(vehicle_inside_partial_attribute_update_1_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_inside_partial_attribute_update_1).send().await?.done()?;
    let patch = Attribute::from(Property::builder(71).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&vehicle_inside_partial_attribute_update_1_id, &"speed".parse::<AttributeName>()?, patch)
        .entity_type(type_selection::entity_type(&vehicle_type).or(&other_type))
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_inside_partial_attribute_update_1_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", property().value(71)));
    let vehicle_inside_delete_attribute_0_id = namespace.urn("Vehicle", "inside-delete-attribute-0")?;
    let vehicle_inside_delete_attribute_0 = Entity::builder()
        .id(vehicle_inside_delete_attribute_0_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_inside_delete_attribute_0).send().await?.done()?;
    let speed_name = "speed".parse::<AttributeName>()?;
    broker
        .entities()
        .delete_attribute(&vehicle_inside_delete_attribute_0_id, &speed_name)
        .entity_type(&vehicle_type)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_inside_delete_attribute_0_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("speed"));
    let vehicle_inside_delete_attribute_1_id = namespace.urn("Vehicle", "inside-delete-attribute-1")?;
    let vehicle_inside_delete_attribute_1 = Entity::builder()
        .id(vehicle_inside_delete_attribute_1_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_inside_delete_attribute_1).send().await?.done()?;
    let speed_name = "speed".parse::<AttributeName>()?;
    broker
        .entities()
        .delete_attribute(&vehicle_inside_delete_attribute_1_id, &speed_name)
        .entity_type(type_selection::entity_type(&vehicle_type).or(&other_type))
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_inside_delete_attribute_1_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("speed"));
    let vehicle_inside_replace_attribute_0_id = namespace.urn("Vehicle", "inside-replace-attribute-0")?;
    let vehicle_inside_replace_attribute_0 = Entity::builder()
        .id(vehicle_inside_replace_attribute_0_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_inside_replace_attribute_0).send().await?.done()?;
    let speed_name = "speed".parse::<AttributeName>()?;
    let attribute = Attribute::from(Property::builder(72).build()?);
    broker
        .entities()
        .replace_attribute(&vehicle_inside_replace_attribute_0_id, &speed_name, &attribute)
        .entity_type(&vehicle_type)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_inside_replace_attribute_0_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", property().value(72)));
    let vehicle_inside_replace_attribute_1_id = namespace.urn("Vehicle", "inside-replace-attribute-1")?;
    let vehicle_inside_replace_attribute_1 = Entity::builder()
        .id(vehicle_inside_replace_attribute_1_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_inside_replace_attribute_1).send().await?.done()?;
    let speed_name = "speed".parse::<AttributeName>()?;
    let attribute = Attribute::from(Property::builder(72).build()?);
    broker
        .entities()
        .replace_attribute(&vehicle_inside_replace_attribute_1_id, &speed_name, &attribute)
        .entity_type(type_selection::entity_type(&vehicle_type).or(&other_type))
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_inside_replace_attribute_1_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", property().value(72)));
    Ok(())
}
