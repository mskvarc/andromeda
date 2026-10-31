//! A single unit code answers every method over one period.

use andromeda_test_suite::{
    aggregation_matcher::{aggregated_as, has_aggregate, period},
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        NgsiLdMediaType,
        Property,
        evolution::{AggregatedKind, AggregationMethod, TemporalEntity},
    },
};

/// Pins: avg, min, max and `totalCount` over Property instances recorded under one `unitCode` answer one triple
/// each for the one period an absent `aggrPeriodDuration` makes, with the mean, the extrema and the count of the
/// observed values
/// Clause: 4.5.19, 5.7.3.4
/// Recorded: with no `aggrPeriodDuration` and no timerel the one period spans an unbounded time range (clause
/// 4.5.19.1, PT0S), which sets no start or end, and clause 4.5.19 makes each bound a `DateTime`; Andromeda bounds
/// the period by the least and the most recent instant the response holds, 2026-01-01T00:00:00Z and 01:00
#[gtest]
#[tokio::test]
async fn o_tmp_agg_0003_a_single_unit_code_answers_every_method_over_one_period() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let scale_id = namespace.urn("Scale", 1)?;
    let scale_type = namespace.entity_type("Scale")?;
    let scale_evolution = TemporalEntity::builder()
        .id(scale_id.clone())
        .entity_type(scale_type)
        .property(
            "weight",
            Property::builder(5).unit_code(UnitCode::Kgm).observed_at("2026-01-01T00:00:00Z".parse()?),
        )
        .property(
            "weight",
            Property::builder(7).unit_code(UnitCode::Kgm).observed_at("2026-01-01T01:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&scale_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&scale_id)
        .aggregation_methods("avg,min,max,totalCount".parse()?)
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(aggregated, aggregated_as("weight", AggregatedKind::Property));
    expect_that!(
        aggregated,
        has_aggregate(
            "weight",
            AggregationMethod::Avg,
            all!(
                len(eq(1)),
                nth(0, period().value(6).start("2026-01-01T00:00:00Z".parse()?).end("2026-01-01T01:00:00Z".parse()?))
            )
        )
    );
    expect_that!(
        aggregated,
        has_aggregate(
            "weight",
            AggregationMethod::Min,
            all!(
                len(eq(1)),
                nth(0, period().value(5).start("2026-01-01T00:00:00Z".parse()?).end("2026-01-01T01:00:00Z".parse()?))
            )
        )
    );
    expect_that!(
        aggregated,
        has_aggregate(
            "weight",
            AggregationMethod::Max,
            all!(
                len(eq(1)),
                nth(0, period().value(7).start("2026-01-01T00:00:00Z".parse()?).end("2026-01-01T01:00:00Z".parse()?))
            )
        )
    );
    expect_that!(
        aggregated,
        has_aggregate(
            "weight",
            AggregationMethod::TotalCount,
            all!(
                len(eq(1)),
                nth(0, period().value(2).start("2026-01-01T00:00:00Z".parse()?).end("2026-01-01T01:00:00Z".parse()?))
            )
        )
    );
    Ok(())
}
