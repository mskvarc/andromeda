//! A typed time or date reads back typed and an untyped string as written.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Date, Entity, NgsiLdMediaType, Time, Value},
    query::temporal_query::TemporalProperty,
};

/// Pins: Retrieve Temporal Evolution answers a typed Time 12:00:00.5Z as the typed 12:00:00.500Z and a typed Date
/// typed, as Retrieve Entity does, and an untyped string in the `DateTime` form as the string written
/// Clause: 4.5.7, 4.6.3, 5.7.3, C.6
#[gtest]
#[tokio::test]
async fn n_tmp_ret_4005_a_typed_time_or_date_reads_back_typed_and_an_untyped_string_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let observation_history_id = namespace.urn("Observation", "history")?;
    let observation_type = namespace.entity_type("Observation")?;
    let observation_history = Entity::builder()
        .id(observation_history_id.clone())
        .entity_type(observation_type)
        .property("readingTime", Value::from("12:00:00.5Z".parse::<Time>()?))
        .property("readingDate", Value::from("2024-06-15".parse::<Date>()?))
        .property("readingLabel", "2024-01-01T00:00:00.250000Z")
        .build()?;
    broker.entities().create(&observation_history).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&observation_history_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "readingTime",
            nth(0, any_attribute().value_member(["type"], "Time").value_member(["@value"], "12:00:00.500Z"))
        )
    );
    expect_that!(
        evolution,
        has_evolution(
            "readingDate",
            nth(0, any_attribute().value_member(["type"], "Date").value_member(["@value"], "2024-06-15"))
        )
    );
    expect_that!(
        evolution,
        has_evolution("readingLabel", nth(0, any_attribute().value("2024-01-01T00:00:00.250000Z")))
    );
    Ok(())
}
