//! A month period runs from time at.

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

/// Pins: sum and `totalCount` with `aggrPeriodDuration` P1M over a between query from 2026-03-15 to 2026-06-15
/// answer three periods, 03-15 to 04-15, 04-15 to 05-15 and 05-15 to 06-15, each holding the two instances on its
/// first and its last day; none starts on the first of a month
/// Clause: 4.5.19, 4.5.19.1
/// Recorded: 4.5.19 asks for "as many array elements as there are periods in the time range of the query", each
/// with "the start `DateTime` of the corresponding period"; a period of one month from `timeAt` starts at `timeAt`,
/// so the periods lie in the time range of the query rather than on calendar months that reach outside it
#[gtest]
#[tokio::test]
async fn n_tmp_agg_9012_a_month_period_runs_from_time_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(1).observed_at("2026-03-15T12:00:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2026-04-14T12:00:00Z".parse()?))
        .property("speed", Property::builder(3).observed_at("2026-04-15T12:00:00Z".parse()?))
        .property("speed", Property::builder(4).observed_at("2026-05-14T12:00:00Z".parse()?))
        .property("speed", Property::builder(5).observed_at("2026-05-15T12:00:00Z".parse()?))
        .property("speed", Property::builder(6).observed_at("2026-06-14T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .temporal_q(
            temporal_query::between("2026-03-15T00:00:00Z".parse()?, "2026-06-15T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Sum, AggregationMethod::TotalCount])
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
                nth(0, period().value(3).start("2026-03-15T00:00:00Z".parse()?).end("2026-04-15T00:00:00Z".parse()?)),
                nth(1, period().value(7).start("2026-04-15T00:00:00Z".parse()?).end("2026-05-15T00:00:00Z".parse()?)),
                nth(
                    2,
                    period().value(11).start("2026-05-15T00:00:00Z".parse()?).end("2026-06-15T00:00:00Z".parse()?)
                )
            )
        )
    );
    expect_that!(
        aggregated,
        has_aggregate(
            "speed",
            AggregationMethod::TotalCount,
            all!(nth(0, period().value(2)), nth(1, period().value(2)), nth(2, period().value(2)))
        )
    );
    Ok(())
}
