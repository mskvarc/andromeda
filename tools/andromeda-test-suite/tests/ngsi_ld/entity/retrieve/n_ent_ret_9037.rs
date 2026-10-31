//! A typed time value is served in its one spelling at every depth.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    simplified_matcher::has_value_at,
    value::object,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{DateTime, Entity, NgsiLdMediaType, Time, Value},
};

/// Pins: Retrieve Entity answers a typed Time 12:00:00.5Z as 12:00:00.500Z and a typed `DateTime` written with a
/// comma fraction as 2024-01-01T00:00:00.500Z, typed, normalized and `keyValues`, at the top level and inside a
/// structured value
/// Clause: 4.6.3, 5.7.1, C.6
/// Recorded: clause 4.6.3 admits several spellings of one time value and fixes none for output; Andromeda serves a
/// declared `DateTime` or Time in one spelling, UTC with Z and the sub-second precision it carries in groups of
/// three digits
#[gtest]
#[tokio::test]
async fn n_ent_ret_9037_a_typed_time_value_is_served_in_its_one_spelling_at_every_depth() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let observation_typed_id = namespace.urn("Observation", "typed")?;
    let observation_type = namespace.entity_type("Observation")?;
    let observation_typed = Entity::builder()
        .id(observation_typed_id.clone())
        .entity_type(observation_type)
        .property("readingTime", Value::from("12:00:00.5Z".parse::<Time>()?))
        .property("readingInstant", Value::from("2024-01-01T00:00:00,5Z".parse::<DateTime>()?))
        .property("reading", object([("at", Value::from("2024-01-01T00:00:00.000Z".parse::<DateTime>()?))]))
        .build()?;
    broker.entities().create(&observation_typed).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&observation_typed_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "readingTime",
            any_attribute().value_member(["type"], "Time").value_member(["@value"], "12:00:00.500Z")
        )
    );
    expect_that!(
        entity,
        has_attribute(
            "readingInstant",
            any_attribute()
                .value_member(["type"], "DateTime")
                .value_member(["@value"], "2024-01-01T00:00:00.500Z")
        )
    );
    expect_that!(
        entity,
        has_attribute(
            "reading",
            any_attribute()
                .value_member(["at", "type"], "DateTime")
                .value_member(["at", "@value"], "2024-01-01T00:00:00Z")
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&observation_typed_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_value_at("readingTime", ["type"], "Time"));
    expect_that!(entity_2, has_value_at("readingTime", ["@value"], "12:00:00.500Z"));
    expect_that!(entity_2, has_value_at("readingInstant", ["type"], "DateTime"));
    expect_that!(entity_2, has_value_at("readingInstant", ["@value"], "2024-01-01T00:00:00.500Z"));
    Ok(())
}
