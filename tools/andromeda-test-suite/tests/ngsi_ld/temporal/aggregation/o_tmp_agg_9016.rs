//! A month period before time at ends at time at.

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

/// Pins: sum with `aggrPeriodDuration` P1M and timerel before 2026-03-15 answers the periods 01-15 to 02-15 and
/// 02-15 to 03-15: the grid reaches back from `timeAt` in whole months, so the last period ends at `timeAt`
/// Clause: 4.5.19, 4.5.19.1, 4.11
/// Recorded: the specification does not say where the periods of a before query are anchored; Andromeda anchors
/// every grid at `timeAt`, which for before is the end of the time range
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9016_a_month_period_before_time_at_ends_at_time_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(1).observed_at("2026-01-20T12:00:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2026-02-20T12:00:00Z".parse()?))
        .property("speed", Property::builder(3).observed_at("2026-03-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .temporal_q(
            temporal_query::before("2026-03-15T00:00:00Z".parse()?)
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
                len(eq(2)),
                nth(0, period().value(1).start("2026-01-15T00:00:00Z".parse()?).end("2026-02-15T00:00:00Z".parse()?)),
                nth(1, period().value(5).start("2026-02-15T00:00:00Z".parse()?).end("2026-03-15T00:00:00Z".parse()?))
            )
        )
    );
    Ok(())
}
