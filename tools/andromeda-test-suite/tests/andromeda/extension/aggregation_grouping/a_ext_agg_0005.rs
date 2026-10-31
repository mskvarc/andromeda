//! A value key groups by the attribute value in effect.

use andromeda_test_suite::{
    broker,
    grouped_aggregation::{GroupedEntity, grouped_request},
    namespace::Namespace,
    outcome::Upserted,
    problem::answered,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{Property, evolution::TemporalEntity};

/// Pins: a grouped temporal retrieve with `aggrGroupBy` naming another attribute answers 200 grouping each instance
/// by the value that attribute had in effect at its `observedAt`
/// Clause: 4.5.19, 6.19.3.1
/// Recorded: Andromeda extension, not v1.9.1: the temporal binding defines neither `aggrGroupBy` nor the
/// `groupedValues` format; the `temporal_aggregation_grouping` extension adds them, here with at most 3 groups
#[gtest]
#[tokio::test]
async fn a_ext_agg_0005_a_value_key_groups_by_the_attribute_value_in_effect() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property(
            "speed",
            Property::builder(10)
                .unit_code(UnitCode::Kmh)
                .dataset_id(namespace.dataset_id("Dataset", "A")?)
                .observed_at("2024-01-01T08:00:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(20)
                .unit_code(UnitCode::Kmh)
                .dataset_id(namespace.dataset_id("Dataset", "A")?)
                .observed_at("2024-02-01T09:00:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(30)
                .unit_code(UnitCode::Hm)
                .dataset_id(namespace.dataset_id("Dataset", "B")?)
                .observed_at("2024-01-15T08:30:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(40)
                .unit_code(UnitCode::Hm)
                .dataset_id(namespace.dataset_id("Dataset", "B")?)
                .observed_at("2024-03-01T10:00:00Z".parse()?),
        )
        .property("zone", Property::builder("north").observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("zone", Property::builder("south").observed_at("2024-02-15T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = grouped_request(
        &broker,
        &vehicle_id,
        "timerel=after&timeAt=1970-01-01T00%3A00%3A00Z&attrs=speed&format=groupedValues&aggrMethods=avg&aggrGroupBy=zone",
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let grouped = GroupedEntity::read(answer.as_ref().or_fail()?)?;
    let speed = grouped.attribute("speed").or_fail()?;
    expect_that!(speed.series_count(), some(eq(2)));
    let dataset_a = speed.series_of(&namespace.uri("Dataset", "A")?).or_fail()?;
    expect_that!(dataset_a.group_value("avg", "north"), some(eq(15.0)));
    expect_that!(dataset_a.group_count("avg"), eq(1));
    let dataset_b = speed.series_of(&namespace.uri("Dataset", "B")?).or_fail()?;
    expect_that!(dataset_b.group_value("avg", "north"), some(eq(30.0)));
    expect_that!(dataset_b.group_value("avg", "south"), some(eq(40.0)));
    expect_that!(dataset_b.group_count("avg"), eq(2));
    Ok(())
}
