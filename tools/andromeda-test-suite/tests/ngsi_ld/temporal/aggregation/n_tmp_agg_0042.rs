//! A single instance aggregates to itself.

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

/// Pins: avg, min, max and sum over one instance of 42 answer 42, and `totalCount` and `distinctCount` answer 1
/// Clause: 4.5.19, 4.5.19.1, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_agg_0042_a_single_instance_aggregates_to_itself() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(42).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .aggregation_methods("avg,min,max,sum,totalCount,distinctCount".parse()?)
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::Avg, nth(0, period().value(42))));
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::Min, nth(0, period().value(42))));
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::Max, nth(0, period().value(42))));
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::Sum, nth(0, period().value(42))));
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::TotalCount, nth(0, period().value(1))));
    expect_that!(aggregated, has_aggregate("speed", AggregationMethod::DistinctCount, nth(0, period().value(1))));
    Ok(())
}
