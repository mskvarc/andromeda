//! Grouping by unit code answers a mixed unit aggregation.

use andromeda_test_suite::{
    broker,
    grouped_aggregation::{GroupedEntity, grouped_request},
    namespace::Namespace,
    outcome::Upserted,
    problem::{answered, problem},
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        ErrorType,
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: an avg over one instance series carrying two `unitCodes` is refused with `BadRequestData` as
/// `aggregatedValues` and answers 200 as `groupedValues` with `aggrGroupBy=unitCode`, one group per `unitCode` each
/// aggregated on its own
/// Clause: 4.5.19, 6.19.3.1
/// Recorded: Andromeda extension, not v1.9.1: the temporal binding defines neither `aggrGroupBy` nor the
/// `groupedValues` format; the `temporal_aggregation_grouping` extension adds them, here with at most 3 groups
#[gtest]
#[tokio::test]
async fn a_ext_agg_0002_grouping_by_unit_code_answers_a_mixed_unit_aggregation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property(
            "speed",
            Property::builder(10).unit_code(UnitCode::Kmh).observed_at("2024-01-01T08:00:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(20).unit_code(UnitCode::Kmh).observed_at("2024-02-01T09:00:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(30).unit_code(UnitCode::Hm).observed_at("2024-03-01T10:00:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(40).unit_code(UnitCode::Hm).observed_at("2024-04-01T11:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&vehicle_id)
        .temporal_q(temporal_query::after("1970-01-01T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Avg]))
        .attrs(vec!["speed".parse()?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = grouped_request(
        &broker,
        &vehicle_id,
        "timerel=after&timeAt=1970-01-01T00%3A00%3A00Z&attrs=speed&format=groupedValues&aggrMethods=avg&aggrGroupBy=unitCode",
    )?;
    let answer_2 = broker.send_unchecked(request).await;
    expect_that!(&answer_2, answered(StatusCode::OK));
    let grouped = GroupedEntity::read(answer_2.as_ref().or_fail()?)?;
    let speed = grouped.attribute("speed").or_fail()?;
    let series = speed.single().or_fail()?;
    expect_that!(series.group_value("avg", "KMH"), some(eq(15.0)));
    expect_that!(series.group_value("avg", "HM"), some(eq(35.0)));
    expect_that!(series.group_count("avg"), eq(2));
    Ok(())
}
