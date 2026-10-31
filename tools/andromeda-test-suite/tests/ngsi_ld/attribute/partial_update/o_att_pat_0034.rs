//! A sub attribute patched with the content it holds keeps both system times.

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

/// Pins: a Partial Attribute Update restating a sub-attribute with the content it already holds moves neither its
/// createdAt nor its modifiedAt
/// Clause: 4.8, 5.6.4.4, 5.7.1.4
/// Recorded: 4.8 does not say whether restating a member with the content it holds is a modification; Andromeda keeps
/// the system times of content it already holds
#[gtest]
#[tokio::test]
async fn o_att_pat_0034_a_sub_attribute_patched_with_the_content_it_holds_keeps_both_system_times() -> Result<()> {
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
    let modified = system_time::of_attribute(entity, &["temperature", "accuracy"], SystemMember::ModifiedAt)?;
    let fragment = AttributeMembers::builder().property("accuracy", Value::Number("0.5".parse()?)).build()?;
    let answer = broker
        .with_responses()
        .entities()
        .update_attribute(&building_id, &"temperature".parse::<AttributeName>()?, fragment)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(
        entity_2,
        has_attribute(
            "temperature",
            property().sub_attribute("accuracy", property().value(Value::Number("0.5".parse()?)))
        )
    );
    expect_that!(
        system_time::of_attribute(entity_2, &["temperature", "accuracy"], SystemMember::CreatedAt)?,
        eq(created)
    );
    expect_that!(
        system_time::of_attribute(entity_2, &["temperature", "accuracy"], SystemMember::ModifiedAt)?,
        eq(modified)
    );
    Ok(())
}
