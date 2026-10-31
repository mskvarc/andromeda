//! A patched sub attribute keeps its created at and moves its modified at.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{AttributeMembers, AttributeName, Entity, NgsiLdMediaType, Property, Value},
};

/// Pins: a sub-attribute carries createdAt and modifiedAt: created with its Attribute both are the write instant,
/// and a Partial Attribute Update of the sub-attribute keeps its createdAt and moves its modifiedAt and the
/// Attribute's to the instant of the update
/// Clause: 4.8, 5.6.4.4, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_att_pat_0033_a_patched_sub_attribute_keeps_its_created_at_and_moves_its_modified_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("temperature", Property::builder(20).property("accuracy", Value::Number("0.5".parse()?)))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let created = system_time::of_attribute(entity, &["temperature", "accuracy"], SystemMember::CreatedAt)?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(
        system_time::of_attribute(entity_2, &["temperature", "accuracy"], SystemMember::ModifiedAt)?,
        eq(created)
    );
    expect_that!(system_time::of_attribute(entity_2, &["temperature"], SystemMember::CreatedAt)?, eq(created));
    let fragment = AttributeMembers::builder().property("accuracy", Value::Number("0.25".parse()?)).build()?;
    let answer = broker
        .with_responses()
        .entities()
        .update_attribute(&building_id, &"temperature".parse::<AttributeName>()?, fragment)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(
        entity_3,
        has_attribute(
            "temperature",
            property().value(20).sub_attribute("accuracy", property().value(Value::Number("0.25".parse()?)))
        )
    );
    expect_that!(
        system_time::of_attribute(entity_3, &["temperature", "accuracy"], SystemMember::CreatedAt)?,
        eq(created)
    );
    expect_that!(
        system_time::of_attribute(entity_3, &["temperature", "accuracy"], SystemMember::ModifiedAt)?,
        not(eq(created))
    );
    let modified = system_time::of_attribute(entity_3, &["temperature", "accuracy"], SystemMember::ModifiedAt)?;
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(system_time::of_attribute(entity_4, &["temperature"], SystemMember::ModifiedAt)?, eq(modified));
    expect_that!(system_time::of_attribute(entity_4, &["temperature"], SystemMember::CreatedAt)?, eq(created));
    Ok(())
}
