//! An hourly period spans exactly one hour.

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

/// Pins: avg with `aggrPeriodDuration` PT1H over instances at 00:10 and 01:10 answers two triples spanning 00:10 to
/// 01:10 and 01:10 to 02:10
/// Clause: 4.5.19, 4.5.19.1, 5.7.3.4
/// Recorded: without a temporal query the spec does not define where the time range and its periods start;
/// Andromeda starts them at the earliest instance
#[gtest]
#[tokio::test]
async fn o_tmp_agg_0045_an_hourly_period_spans_exactly_one_hour() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:10:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-01T01:10:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .aggregation_methods("avg".parse()?)
        .aggregation_period("PT1H".parse()?)
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(
            "speed",
            AggregationMethod::Avg,
            all!(
                len(eq(2)),
                nth(0, period().start("2024-01-01T00:10:00Z".parse()?).end("2024-01-01T01:10:00Z".parse()?)),
                nth(1, period().start("2024-01-01T01:10:00Z".parse()?).end("2024-01-01T02:10:00Z".parse()?))
            )
        )
    );
    Ok(())
}
