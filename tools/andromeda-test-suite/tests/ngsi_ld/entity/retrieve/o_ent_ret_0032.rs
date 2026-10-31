//! A date time value reads back in its sub second form.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType, Property, Value},
};

/// Pins: Retrieve Entity answers a `DateTime` value with no fractional digits, three or six: a fraction written
/// shorter comes back padded to milliseconds, one written to milliseconds or microseconds comes back as written, at
/// the top level and as a sub-attribute
/// Clause: 4.6.3, 4.6.4, 5.7.1.4
/// Recorded: 4.6.3 admits any number of fractional digits; Andromeda holds a `DateTime` value as its instant and
/// answers it with 0, 3 or 6 fractional digits, as its temporal representation does
#[gtest]
#[tokio::test]
async fn o_ent_ret_0032_a_date_time_value_reads_back_in_its_sub_second_form() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("short", Value::from("2024-01-01T00:00:00.5Z".parse::<DateTime>()?))
        .property("milli", Value::from("2024-01-01T00:00:00.500Z".parse::<DateTime>()?))
        .property("micro", Value::from("2024-01-01T00:00:00.123456Z".parse::<DateTime>()?))
        .property("whole", Value::from("2024-01-01T00:00:00Z".parse::<DateTime>()?))
        .property(
            "opening",
            Property::builder(1).property("at", Value::from("2024-01-01T00:00:00.5Z".parse::<DateTime>()?)),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "short",
            any_attribute().value_member_as_written([], Value::from("2024-01-01T00:00:00.500Z".parse::<DateTime>()?))
        )
    );
    expect_that!(
        entity,
        has_attribute(
            "milli",
            any_attribute().value_member_as_written([], Value::from("2024-01-01T00:00:00.500Z".parse::<DateTime>()?))
        )
    );
    expect_that!(
        entity,
        has_attribute(
            "micro",
            any_attribute().value_member_as_written([], Value::from("2024-01-01T00:00:00.123456Z".parse::<DateTime>()?))
        )
    );
    expect_that!(
        entity,
        has_attribute(
            "whole",
            any_attribute().value_member_as_written([], Value::from("2024-01-01T00:00:00Z".parse::<DateTime>()?))
        )
    );
    expect_that!(
        entity,
        has_attribute(
            "opening",
            any_attribute().sub_attribute(
                "at",
                any_attribute().value_member_as_written([], Value::from("2024-01-01T00:00:00.500Z".parse::<DateTime>()?))
            )
        )
    );
    Ok(())
}
