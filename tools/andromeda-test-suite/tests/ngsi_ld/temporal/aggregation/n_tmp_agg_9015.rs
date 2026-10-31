//! A year period runs from time at.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: sum and `totalCount` with `aggrPeriodDuration` P1Y over a between query from 2024-06-15 to 2026-06-15
/// answer two periods, 2024-06-15 to 2025-06-15 (two instances) and 2025-06-15 to 2026-06-15 (one), neither aligned
/// on the first of a month
/// Clause: 4.5.19, 4.5.19.1
/// Recorded: a period of one year from `timeAt` starts at `timeAt`, so the periods lie in the time range of the
/// query
#[gtest]
#[tokio::test]
async fn n_tmp_agg_9015_a_year_period_runs_from_time_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(1).observed_at("2024-07-01T12:00:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2025-03-01T12:00:00Z".parse()?))
        .property("speed", Property::builder(4).observed_at("2026-01-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .temporal_q(
            temporal_query::between("2024-06-15T00:00:00Z".parse()?, "2026-06-15T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Sum, AggregationMethod::TotalCount])
                .aggregation_period("P1Y".parse()?),
        )
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(
            "speed",
            AggregationMethod::Sum,
            all!(
                len(eq(2)),
                nth(0, period().value(3).start("2024-06-15T00:00:00Z".parse()?).end("2025-06-15T00:00:00Z".parse()?)),
                nth(1, period().value(4).start("2025-06-15T00:00:00Z".parse()?).end("2026-06-15T00:00:00Z".parse()?))
            )
        )
    );
    expect_that!(
        aggregated,
        has_aggregate(
            "speed",
            AggregationMethod::TotalCount,
            all!(nth(0, period().value(2)), nth(1, period().value(1)))
        )
    );
    Ok(())
}
