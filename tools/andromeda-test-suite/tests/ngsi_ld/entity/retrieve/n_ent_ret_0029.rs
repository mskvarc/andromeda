//! Sys attrs answers created at and modified at at every depth.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, NgsiLdMediaType, Property, Relationship, Value},
};

/// Pins: Retrieve Entity with `options=sysAttrs` answers `createdAt` and `modifiedAt` on every Attribute and on
/// every sub-attribute at every depth below it, all the instant of the create that wrote them
/// Clause: 4.8, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_0029_sys_attrs_answers_created_at_and_modified_at_at_every_depth() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let person_id = namespace.urn("Person", 1)?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property(
            "temperature",
            Property::builder(21).property("accuracy", Property::builder(Value::Number("0.5".parse()?)).property("method", "probe")),
        )
        .relationship("owner", Relationship::builder(person_id).property("since", "2020"))
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
    let created = system_time::of_attribute(entity, &["temperature"], SystemMember::CreatedAt)?;
    expect_that!(
        system_time::of_attribute(entity, &["temperature"], SystemMember::CreatedAt).ok(),
        some(anything())
    );
    expect_that!(system_time::of_attribute(entity, &["temperature"], SystemMember::CreatedAt)?, eq(created));
    expect_that!(system_time::of_attribute(entity, &["temperature"], SystemMember::ModifiedAt)?, eq(created));
    expect_that!(
        system_time::of_attribute(entity, &["temperature", "accuracy"], SystemMember::CreatedAt)?,
        eq(created)
    );
    expect_that!(
        system_time::of_attribute(entity, &["temperature", "accuracy"], SystemMember::ModifiedAt)?,
        eq(created)
    );
    expect_that!(
        system_time::of_attribute(entity, &["temperature", "accuracy", "method"], SystemMember::CreatedAt)?,
        eq(created)
    );
    expect_that!(
        system_time::of_attribute(entity, &["temperature", "accuracy", "method"], SystemMember::ModifiedAt)?,
        eq(created)
    );
    expect_that!(system_time::of_attribute(entity, &["owner"], SystemMember::CreatedAt)?, eq(created));
    expect_that!(system_time::of_attribute(entity, &["owner"], SystemMember::ModifiedAt)?, eq(created));
    expect_that!(system_time::of_attribute(entity, &["owner", "since"], SystemMember::CreatedAt)?, eq(created));
    expect_that!(system_time::of_attribute(entity, &["owner", "since"], SystemMember::ModifiedAt)?, eq(created));
    expect_that!(
        entity,
        has_attribute(
            "temperature",
            any_attribute().sub_attribute("accuracy", any_attribute().sub_attribute("method", any_attribute().value("probe")))
        )
    );
    expect_that!(
        entity,
        has_attribute("owner", any_attribute().sub_attribute("since", any_attribute().value("2020")))
    );
    Ok(())
}
