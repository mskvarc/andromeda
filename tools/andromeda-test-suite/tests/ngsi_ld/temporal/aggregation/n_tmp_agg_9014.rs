//! A period mixing months and days is answered.

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

/// Pins: sum with `aggrPeriodDuration` P1M1D, a duration mixing a month with a day, answers 200 with the periods
/// 01-31 to 03-01, 03-01 to 04-02 and 04-02 to 05-03 from `timeAt` 2026-01-31; the last period keeps its grid end
/// past `endTimeAt` and holds only the instances inside the window
/// Clause: 4.5.19, 4.5.19.1
/// Recorded: 4.5.19.1 writes the duration as P`[n]`Y`[n]`M`[n]`DT`[n]`H`[n]`M`[n]`S with only zero elements
/// omitted, so a month and a day together are a valid duration; period k runs from `timeAt` plus k times one month
/// and one day
#[gtest]
#[tokio::test]
async fn n_tmp_agg_9014_a_period_mixing_months_and_days_is_answered() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(1).observed_at("2026-01-31T12:00:00Z".parse()?))
        .property("speed", Property::builder(2).observed_at("2026-02-28T12:00:00Z".parse()?))
        .property("speed", Property::builder(3).observed_at("2026-03-01T12:00:00Z".parse()?))
        .property("speed", Property::builder(4).observed_at("2026-04-01T12:00:00Z".parse()?))
        .property("speed", Property::builder(5).observed_at("2026-04-02T12:00:00Z".parse()?))
        .property("speed", Property::builder(6).observed_at("2026-04-29T12:00:00Z".parse()?))
        .property("speed", Property::builder(100).observed_at("2026-05-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .temporal_q(
            temporal_query::between("2026-01-31T00:00:00Z".parse()?, "2026-04-30T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Sum])
                .aggregation_period("P1M1D".parse()?),
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
                nth(0, period().value(3).start("2026-01-31T00:00:00Z".parse()?).end("2026-03-01T00:00:00Z".parse()?)),
                nth(1, period().value(7).start("2026-03-01T00:00:00Z".parse()?).end("2026-04-02T00:00:00Z".parse()?)),
                nth(
                    2,
                    period().value(11).start("2026-04-02T00:00:00Z".parse()?).end("2026-05-03T00:00:00Z".parse()?)
                )
            )
        )
    );
    Ok(())
}
