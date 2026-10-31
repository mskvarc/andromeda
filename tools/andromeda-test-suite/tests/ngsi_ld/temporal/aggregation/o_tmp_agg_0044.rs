//! All eight methods of numbers.

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

/// Pins: all eight methods over the numbers 10, 20 and 30 answer avg 20, min 10, max 30, sum 60, sumsq 1400, stddev
/// 10, `totalCount` 3 and `distinctCount` 3
/// Clause: 4.5.19, 4.5.19.1, 5.7.3.4
/// Recorded: 4.5.19.1 asks for "the standard deviation" without choosing the sample or the population form;
/// Andromeda answers the sample standard deviation
#[gtest]
#[tokio::test]
async fn o_tmp_agg_0044_all_eight_methods_of_numbers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(10).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("speed", Property::builder(30).observed_at("2024-01-01T02:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .aggregation_methods("avg,min,max,sum,sumsq,stddev,totalCount,distinctCount".parse()?)
        .send()
        .await?;
    let aggregated = retrieved.body();
    for (method, expected) in [
        (AggregationMethod::Avg, 20),
        (AggregationMethod::Min, 10),
        (AggregationMethod::Max, 30),
        (AggregationMethod::Sum, 60),
        (AggregationMethod::Sumsq, 1400),
        (AggregationMethod::TotalCount, 3),
        (AggregationMethod::DistinctCount, 3),
    ] {
        expect_that!(aggregated, has_aggregate("speed", method, nth(0, period().value(expected))));
    }
    expect_that!(
        aggregated,
        has_aggregate("speed", AggregationMethod::Stddev, nth(0, period().value_between(9.99, 10.01)))
    );
    Ok(())
}
