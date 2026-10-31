//! An array in a sub attribute round trips.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: An observation whose sources sub-Property holds `["x", "y"]` reads back with that array from Retrieve Temporal
/// Evolution and from the current representation
/// Clause: 5.2.5, 4.5.2, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_trl_9007_an_array_in_a_sub_attribute_round_trips() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let array_probe_id = namespace.urn("ArrayProbe", 1)?;
    let array_probe_type = namespace.entity_type("ArrayProbe")?;
    let array_probe_evolution = TemporalEntity::builder()
        .id(array_probe_id.clone())
        .entity_type(array_probe_type)
        .property(
            "speed",
            Property::builder(1)
                .observed_at("2024-01-01T00:00:00Z".parse()?)
                .property("sources", Value::Array(vec![Value::from("x"), Value::from("y")])),
        )
        .build()?;
    broker.temporal().create_or_update(&array_probe_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&array_probe_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_evolution(
            "speed",
            nth(
                0,
                any_attribute()
                    .value(1)
                    .sub_attribute("sources", property().length_at([], 2).value_member(["0"], "x").value_member(["1"], "y"))
            )
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&array_probe_id)
        .send()
        .await?;
    expect_that!(
        retrieved_2.body(),
        has_attribute(
            "speed",
            any_attribute().sub_attribute("sources", property().length_at([], 2).value_member(["0"], "x").value_member(["1"], "y"))
        )
    );
    Ok(())
}
