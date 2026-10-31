//! A list property renders as value lists.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Upserted,
    simplified_pairs::{list_pairs, property_pairs},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{DateTime, ListProperty, NgsiLdMediaType, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Retrieve Temporal Evolution with temporalValues of a `ListProperty` with two instances answers type `ListProperty`
/// and `valueLists` pairing each ordered `valueList` with its observedAt, and neither values nor value
/// Clause: 4.5.9, 4.5.21.2
#[gtest]
#[tokio::test]
async fn n_tmp_trl_9009_a_list_property_renders_as_value_lists() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_value_lists_id = namespace.urn("Building", "value-lists")?;
    let building_type = namespace.entity_type("Building")?;
    let first_day = "2022-08-09T18:25:02Z".parse::<DateTime>()?;
    let second_day = "2022-08-10T18:25:02Z".parse::<DateTime>()?;
    let building_value_lists_evolution = TemporalEntity::builder()
        .id(building_value_lists_id.clone())
        .entity_type(building_type)
        .list_property(
            "period",
            ListProperty::builder(vec![Value::from("First"), Value::from("Second")]).observed_at(first_day),
        )
        .list_property(
            "period",
            ListProperty::builder(vec![Value::from("1st"), Value::from("2nd")]).observed_at(second_day),
        )
        .build()?;
    broker.temporal().create_or_update(&building_value_lists_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_values(&building_value_lists_id)
        .temporal_q(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    let values = retrieved.body();
    let pairs = list_pairs(values, "period").or_fail()?;
    expect_that!(pairs, len(eq(2)));
    let first = pairs.iter().find(|pair| pair.time.timestamp() == first_day.timestamp()).or_fail()?;
    expect_that!(&first.value, eq(&vec![Value::from("First"), Value::from("Second")]));
    let second = pairs.iter().find(|pair| pair.time.timestamp() == second_day.timestamp()).or_fail()?;
    expect_that!(&second.value, eq(&vec![Value::from("1st"), Value::from("2nd")]));
    expect_that!(property_pairs(values, "period"), none());
    Ok(())
}
