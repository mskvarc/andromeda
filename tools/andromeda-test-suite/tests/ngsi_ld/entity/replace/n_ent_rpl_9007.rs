//! A write whose type excludes the entity is not found.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, EntityFragment, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Delete Entity, Replace Entity, Append, Update, Partial Attribute update, Delete Attribute and Replace
/// Attribute with type naming another type each answer 404 `ResourceNotFound`, and the Vehicle reads back as before
/// Clause: 4.17, Tables 6.5.3.2-1 to 6.7.3.3-1, 5.6.2.4 to 5.6.6.4, 5.6.18.4, 5.6.19.4
#[gtest]
#[tokio::test]
async fn n_ent_rpl_9007_a_write_whose_type_excludes_the_entity_is_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let other_type = namespace.entity_type("Other")?;
    let vehicle_outside_delete_entity_id = namespace.urn("Vehicle", "outside-delete-entity")?;
    let vehicle_outside_delete_entity = Entity::builder()
        .id(vehicle_outside_delete_entity_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_outside_delete_entity).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_outside_delete_entity_id)
        .send()
        .await?;
    let entity = retrieved.body();
    let answer_2 = broker
        .with_responses()
        .entities()
        .delete(&vehicle_outside_delete_entity_id)
        .entity_type(&other_type)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_outside_delete_entity_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, eq(entity));
    let vehicle_outside_replace_entity_id = namespace.urn("Vehicle", "outside-replace-entity")?;
    let vehicle_outside_replace_entity = Entity::builder()
        .id(vehicle_outside_replace_entity_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_outside_replace_entity).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_outside_replace_entity_id)
        .send()
        .await?;
    let entity = retrieved.body();
    let replacement = Entity::builder()
        .id(vehicle_outside_replace_entity_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    let answer_2 = broker.with_responses().entities().replace(&replacement).entity_type(&other_type).send().await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_outside_replace_entity_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, eq(entity));
    let vehicle_outside_append_attributes_id = namespace.urn("Vehicle", "outside-append-attributes")?;
    let vehicle_outside_append_attributes = Entity::builder()
        .id(vehicle_outside_append_attributes_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_outside_append_attributes).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_outside_append_attributes_id)
        .send()
        .await?;
    let entity = retrieved.body();
    let fragment_2 = EntityFragment::builder().property("color", "red").build()?;
    let answer_2 = broker
        .with_responses()
        .entities()
        .append_attributes(&vehicle_outside_append_attributes_id, &fragment_2)
        .entity_type(&other_type)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_outside_append_attributes_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, eq(entity));
    let vehicle_outside_update_attributes_id = namespace.urn("Vehicle", "outside-update-attributes")?;
    let vehicle_outside_update_attributes = Entity::builder()
        .id(vehicle_outside_update_attributes_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_outside_update_attributes).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_outside_update_attributes_id)
        .send()
        .await?;
    let entity = retrieved.body();
    let fragment_2 = EntityFragment::builder().property("speed", 70).build()?;
    let answer_2 = broker
        .with_responses()
        .entities()
        .update_attributes(&vehicle_outside_update_attributes_id, &fragment_2)
        .entity_type(&other_type)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_outside_update_attributes_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, eq(entity));
    let vehicle_outside_partial_attribute_update_id = namespace.urn("Vehicle", "outside-partial-attribute-update")?;
    let vehicle_outside_partial_attribute_update = Entity::builder()
        .id(vehicle_outside_partial_attribute_update_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_outside_partial_attribute_update).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_outside_partial_attribute_update_id)
        .send()
        .await?;
    let entity = retrieved.body();
    let patch = Attribute::from(Property::builder(71).build()?).into_patch();
    let answer_2 = broker
        .with_responses()
        .entities()
        .update_attribute(&vehicle_outside_partial_attribute_update_id, &"speed".parse::<AttributeName>()?, patch)
        .entity_type(&other_type)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_outside_partial_attribute_update_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, eq(entity));
    let vehicle_outside_delete_attribute_id = namespace.urn("Vehicle", "outside-delete-attribute")?;
    let vehicle_outside_delete_attribute = Entity::builder()
        .id(vehicle_outside_delete_attribute_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_outside_delete_attribute).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_outside_delete_attribute_id)
        .send()
        .await?;
    let entity = retrieved.body();
    let speed_name_2 = "speed".parse::<AttributeName>()?;
    let answer_2 = broker
        .with_responses()
        .entities()
        .delete_attribute(&vehicle_outside_delete_attribute_id, &speed_name_2)
        .entity_type(&other_type)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_outside_delete_attribute_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, eq(entity));
    let vehicle_outside_replace_attribute_id = namespace.urn("Vehicle", "outside-replace-attribute")?;
    let vehicle_outside_replace_attribute = Entity::builder()
        .id(vehicle_outside_replace_attribute_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 60)
        .property("plate", "ABC")
        .build()?;
    broker.entities().create(&vehicle_outside_replace_attribute).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_outside_replace_attribute_id)
        .send()
        .await?;
    let entity = retrieved.body();
    let speed_name_2 = "speed".parse::<AttributeName>()?;
    let attribute_2 = Attribute::from(Property::builder(72).build()?);
    let answer_2 = broker
        .with_responses()
        .entities()
        .replace_attribute(&vehicle_outside_replace_attribute_id, &speed_name_2, &attribute_2)
        .entity_type(&other_type)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_outside_replace_attribute_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, eq(entity));
    Ok(())
}
