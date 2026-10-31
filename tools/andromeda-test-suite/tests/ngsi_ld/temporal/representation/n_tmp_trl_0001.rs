//! A json property reads back normalized and simplified.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Upserted,
    simplified_pairs::json_pairs,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{AttributeKind, DateTime, JsonProperty, NgsiLdMediaType, RawJson, evolution::TemporalEntity},
};
use serde_json::json;

/// Pins: a Temporal Evolution created with two `JsonProperty` instances answers 201 and reads back normalized as
/// `JsonProperty` instances holding their json, and simplified as a `jsons` member of `[{"json": ...}, observedAt]` pairs
/// Clause: 5.6.11.4, 4.5.7, 4.5.9, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_trl_0001_a_json_property_reads_back_normalized_and_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let fast = r#"{"maxSpeed": 200, "eco": true}"#.parse::<RawJson>()?;
    let slow = r#"{"maxSpeed": 180, "eco": false}"#.parse::<RawJson>()?;
    let noon = "2020-08-01T12:00:00Z".parse::<DateTime>()?;
    let one = "2020-08-01T13:00:00Z".parse::<DateTime>()?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .json_property("config", JsonProperty::builder(fast.clone()).observed_at(noon))
        .json_property("config", JsonProperty::builder(slow.clone()).observed_at(one))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "config",
            all!(
                len(eq(2)),
                contains(
                    attribute_of(AttributeKind::JsonProperty)
                        .json(json!({"maxSpeed": 200, "eco": true}))
                        .observed_at(noon)
                ),
                contains(
                    attribute_of(AttributeKind::JsonProperty)
                        .json(json!({"maxSpeed": 180, "eco": false}))
                        .observed_at(one)
                )
            )
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_values(&vehicle_id)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    let values = retrieved_2.body();
    let pairs = json_pairs(values, "config").or_fail()?;
    expect_that!(pairs, len(eq(2)));
    let first = pairs
        .iter()
        .find(|pair| same_json(&pair.value, &json!({"maxSpeed": 200, "eco": true})))
        .or_fail()?;
    expect_that!(first.time.timestamp(), eq(noon.timestamp()));
    let second = pairs
        .iter()
        .find(|pair| same_json(&pair.value, &json!({"maxSpeed": 180, "eco": false})))
        .or_fail()?;
    expect_that!(second.time.timestamp(), eq(one.timestamp()));
    Ok(())
}

/// Whether `actual` holds the JSON `expected`, whatever the order of an object's members.
fn same_json(actual: &RawJson, expected: &serde_json::Value) -> bool {
    serde_json::from_str::<serde_json::Value>(&actual.to_string()).is_ok_and(|parsed| &parsed == expected)
}
