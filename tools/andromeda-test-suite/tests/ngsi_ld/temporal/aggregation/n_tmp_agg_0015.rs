//! Avg of booleans counts true as one.

use andromeda_test_suite::{aggregation_matcher::has_aggregate, broker, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        NgsiLdMediaType,
        Property,
        Value,
        evolution::{AggregatedValue, AggregationMethod, TemporalEntity},
    },
};

/// Pins: avg over the booleans true, false and true answers two thirds, true counting 1 and false 0
/// Clause: 4.5.19, 4.5.19.1, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_agg_0015_avg_of_booleans_counts_true_as_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("isActive", Property::builder(true).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("isActive", Property::builder(false).observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("isActive", Property::builder(true).observed_at("2024-01-01T02:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .aggregation_methods("avg".parse()?)
        .send()
        .await?;
    let near_two_thirds = |period: &AggregatedValue| match &period.value {
        Value::Number(number) => number.as_str().parse::<f64>().is_ok_and(|average| (average - 2.0 / 3.0).abs() < 0.01),
        Value::Boolean(_) | Value::Text(_) | Value::Array(_) | Value::Object(_) | Value::Typed(_) | Value::JsonNull => false,
    };
    expect_that!(
        retrieved.body(),
        has_aggregate("isActive", AggregationMethod::Avg, nth(0, predicate(near_two_thirds)))
    );
    Ok(())
}
