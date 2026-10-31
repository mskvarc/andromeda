//! A cyclic hour of day bucket folds the days.

use andromeda_test_suite::{
    broker,
    grouped_aggregation::{GroupedAttribute, GroupedEntity, grouped_request},
    namespace::Namespace,
    outcome::Upserted,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{Property, evolution::TemporalEntity};

/// Pins: a grouped temporal retrieve with aggrGroupBy=observedAt;hourOfDay answers 200 with one group per hour of
/// the day, folding instances of different days observed in the same hour into one group
/// Clause: 4.5.19, 6.19.3.1
/// Recorded: Andromeda extension, not v1.9.1: the temporal binding defines neither `aggrGroupBy` nor the
/// `groupedValues` format; the `temporal_aggregation_grouping` extension adds them, here with at most 3 groups
#[gtest]
#[tokio::test]
async fn a_ext_agg_0004_a_cyclic_hour_of_day_bucket_folds_the_days() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(10).observed_at("2024-01-01T08:00:00Z".parse()?))
        .property("speed", Property::builder(30).observed_at("2024-03-05T08:15:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-02T09:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = grouped_request(
        &broker,
        &vehicle_id,
        "timerel=after&timeAt=1970-01-01T00%3A00%3A00Z&attrs=speed&format=groupedValues&aggrMethods=avg&aggrGroupBy=observedAt%3BhourOfDay",
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let grouped = GroupedEntity::read(answer.as_ref().or_fail()?)?;
    let series = grouped.attribute("speed").and_then(GroupedAttribute::single).or_fail()?;
    expect_that!(series.group_value("avg", "8"), some(eq(20.0)));
    expect_that!(series.group_value("avg", "9"), some(eq(20.0)));
    expect_that!(series.group_count("avg"), eq(2));
    Ok(())
}
