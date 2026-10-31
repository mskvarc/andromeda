//! No period spans the least to the most recent instant.

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
};

/// Pins: avg without `aggrPeriodDuration` or timerel answers one triple, 200, from the least recent instance's
/// instant 2024-01-01T00:00:00Z to the most recent one's 2024-01-01T02:00:00Z
/// Clause: 4.5.19, 4.5.19.1, 5.7.3.4
/// Recorded: without `aggrPeriodDuration` or a temporal query the time range has no bounds the spec defines, and
/// clause 4.5.19 makes each bound a `DateTime`; Andromeda bounds the one period by the least and the most recent
/// instant the response holds, the range clause 6.3.10 names
#[gtest]
#[tokio::test]
async fn o_tmp_agg_0034_no_period_spans_the_least_to_the_most_recent_instant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(100).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(200).observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("speed", Property::builder(300).observed_at("2024-01-01T02:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .aggregation_methods("avg".parse()?)
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(
            "speed",
            AggregationMethod::Avg,
            all!(
                len(eq(1)),
                nth(
                    0,
                    period().value(200).start("2024-01-01T00:00:00Z".parse()?).end("2024-01-01T02:00:00Z".parse()?)
                )
            )
        )
    );
    Ok(())
}
