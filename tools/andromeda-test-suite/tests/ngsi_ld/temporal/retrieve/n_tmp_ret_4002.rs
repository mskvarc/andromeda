//! A typed array element keeps its type.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType, Value},
    query::temporal_query::TemporalProperty,
};

/// Pins: a Property whose value is an array holding a typed value object reads back with that object, type
/// included, from Retrieve Entity and from Retrieve Temporal Evolution
/// Clause: 4.5.2, 4.5.7
/// Recorded: 4.5.7 represents each instance of a Temporal Property as the Property it was, so the instance value is
/// the value written, as the current state already returns it
#[gtest]
#[tokio::test]
async fn n_tmp_ret_4002_a_typed_array_element_keeps_its_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property(
            "events",
            Value::Array(vec![Value::from("a"), Value::from("2024-01-01T00:00:00Z".parse::<DateTime>()?)]),
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
            "events",
            any_attribute()
                .value_member(["1", "@value"], "2024-01-01T00:00:00Z")
                .value_member(["1", "type"], "DateTime")
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(
        evolution,
        has_evolution(
            "events",
            nth(
                0,
                any_attribute()
                    .value_member(["0"], "a")
                    .value_member(["1", "@value"], "2024-01-01T00:00:00Z")
                    .value_member(["1", "type"], "DateTime")
            )
        )
    );
    Ok(())
}
