//! Every system time is spelled in one minimal form at every depth.

use andromeda_test_suite::{
    broker,
    instance_time::instance_time_at,
    namespace::Namespace,
    outcome::{Done, Upserted},
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
};

/// The spelling of an instant the broker emits: UTC, `Z`, and no fraction, three digits or six digits not ending in 000.
const MINIMAL_FORM: &str = r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{3}|\.\d{3}([1-9]\d{2}|\d[1-9]\d|\d{2}[1-9]))?Z$";

/// Pins: Retrieve Entity and Retrieve Temporal Evolution with `options=sysAttrs` spell every `createdAt` and
/// `modifiedAt`, on the Entity, an Attribute, a sub-attribute and a temporal instance, in one form: UTC, Z, and no
/// fraction, three digits or six digits not ending in 000
/// Clause: 4.6.3, 4.8, 5.7.1.4, 5.7.3
/// Recorded: Andromeda spells every instant it emits with the shortest of 0, 3 or 6 fractional digits that holds
/// its microsecond; clause 4.6.3 admits one to six digits and fixes no single spelling
#[gtest]
#[tokio::test]
async fn o_ent_ret_9036_every_system_time_is_spelled_in_one_minimal_form_at_every_depth() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building_1_id = namespace.urn("Building", 1)?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_1 = Entity::builder()
        .id(building_1_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Property::builder(21).property("accuracy", Value::Number("0.5".parse()?)))
        .build()?;
    broker.entities().create(&building_1).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_1_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        system_time::of_entity(entity, SystemMember::CreatedAt)?.to_string(),
        matches_regex(MINIMAL_FORM)
    );
    expect_that!(
        system_time::of_entity(entity, SystemMember::ModifiedAt)?.to_string(),
        matches_regex(MINIMAL_FORM)
    );
    expect_that!(
        system_time::of_attribute(entity, &["temperature"], SystemMember::CreatedAt)?.to_string(),
        matches_regex(MINIMAL_FORM)
    );
    expect_that!(
        system_time::of_attribute(entity, &["temperature"], SystemMember::ModifiedAt)?.to_string(),
        matches_regex(MINIMAL_FORM)
    );
    expect_that!(
        system_time::of_attribute(entity, &["temperature", "accuracy"], SystemMember::CreatedAt)?.to_string(),
        matches_regex(MINIMAL_FORM)
    );
    expect_that!(
        system_time::of_attribute(entity, &["temperature", "accuracy"], SystemMember::ModifiedAt)?.to_string(),
        matches_regex(MINIMAL_FORM)
    );
    let building_2 = TemporalEntity::builder()
        .id(building_2_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Property::builder(21).observed_at("2026-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&building_2).send().await?.created()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_2_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(
        instance_time_at(evolution, &"temperature".parse()?, 0, SystemMember::CreatedAt)?.to_string(),
        matches_regex(MINIMAL_FORM)
    );
    expect_that!(
        instance_time_at(evolution, &"temperature".parse()?, 0, SystemMember::ModifiedAt)?.to_string(),
        matches_regex(MINIMAL_FORM)
    );
    Ok(())
}
