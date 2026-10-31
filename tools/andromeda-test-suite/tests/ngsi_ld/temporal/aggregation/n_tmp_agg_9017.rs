//! An exact period off the hour runs from time at.

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

/// Pins: `totalCount` with `aggrPeriodDuration` PT1H30M over a between query from 00:00 to 04:30, with an instance
/// every 30 minutes from 00:00 to 04:00, answers three periods starting 00:00, 01:30 and 03:00, three instances
/// each
/// Clause: 4.5.19, 4.5.19.1
/// Recorded: a duration of hours and minutes lasts the same in every period, and its periods start at `timeAt`
#[gtest]
#[tokio::test]
async fn n_tmp_agg_9017_an_exact_period_off_the_hour_runs_from_time_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(1).observed_at("2026-03-15T00:00:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2026-03-15T00:30:00Z".parse()?))
        .property("speed", Property::builder(3).observed_at("2026-03-15T01:00:00Z".parse()?))
        .property("speed", Property::builder(4).observed_at("2026-03-15T01:30:00Z".parse()?))
        .property("speed", Property::builder(5).observed_at("2026-03-15T02:00:00Z".parse()?))
        .property("speed", Property::builder(6).observed_at("2026-03-15T02:30:00Z".parse()?))
        .property("speed", Property::builder(7).observed_at("2026-03-15T03:00:00Z".parse()?))
        .property("speed", Property::builder(8).observed_at("2026-03-15T03:30:00Z".parse()?))
        .property("speed", Property::builder(9).observed_at("2026-03-15T04:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .temporal_q(
            temporal_query::between("2026-03-15T00:00:00Z".parse()?, "2026-03-15T04:30:00Z".parse()?)
                .aggregation_methods([AggregationMethod::TotalCount])
                .aggregation_period("PT1H30M".parse()?),
        )
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(
            "speed",
            AggregationMethod::TotalCount,
            all!(
                len(eq(3)),
                nth(0, period().value(3).start("2026-03-15T00:00:00Z".parse()?).end("2026-03-15T01:30:00Z".parse()?)),
                nth(1, period().value(3).start("2026-03-15T01:30:00Z".parse()?).end("2026-03-15T03:00:00Z".parse()?)),
                nth(2, period().value(3).start("2026-03-15T03:00:00Z".parse()?).end("2026-03-15T04:30:00Z".parse()?))
            )
        )
    );
    Ok(())
}
