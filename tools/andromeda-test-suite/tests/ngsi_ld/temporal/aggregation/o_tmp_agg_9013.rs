//! A month period from a month end counts each boundary from time at.

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

/// Pins: sum with `aggrPeriodDuration` P1M from 2026-01-31 answers 01-31 to 02-28, 02-28 to 03-31 and 03-31 to
/// 04-30: each boundary is `timeAt` plus whole months, clamped to the month's last day, so the grid returns to the
/// 31st in March, and the instance of 03-30 belongs to the period starting 02-28
/// Clause: 4.5.19, 4.5.19.1
/// Recorded: the specification leaves the length of a month from a month end to the implementation; Andromeda
/// counts period k from `timeAt` (`timeAt` plus k months, the day clamped), never from the previous boundary, which
/// would carry the clamp forward (02-28, 03-28, 04-28) and shift every later period
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9013_a_month_period_from_a_month_end_counts_each_boundary_from_time_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(1).observed_at("2026-01-31T12:00:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2026-02-27T12:00:00Z".parse()?))
        .property("speed", Property::builder(3).observed_at("2026-02-28T12:00:00Z".parse()?))
        .property("speed", Property::builder(4).observed_at("2026-03-30T12:00:00Z".parse()?))
        .property("speed", Property::builder(5).observed_at("2026-03-31T12:00:00Z".parse()?))
        .property("speed", Property::builder(6).observed_at("2026-04-29T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .temporal_q(
            temporal_query::between("2026-01-31T00:00:00Z".parse()?, "2026-04-30T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Sum])
                .aggregation_period("P1M".parse()?),
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
                len(eq(3)),
                nth(0, period().value(3).start("2026-01-31T00:00:00Z".parse()?).end("2026-02-28T00:00:00Z".parse()?)),
                nth(1, period().value(7).start("2026-02-28T00:00:00Z".parse()?).end("2026-03-31T00:00:00Z".parse()?)),
                nth(
                    2,
                    period().value(11).start("2026-03-31T00:00:00Z".parse()?).end("2026-04-30T00:00:00Z".parse()?)
                )
            )
        )
    );
    Ok(())
}
