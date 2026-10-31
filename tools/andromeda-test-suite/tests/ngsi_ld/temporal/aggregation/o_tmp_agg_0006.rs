//! Decimal aggregates keep digits beyond binary64.

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
        Value,
        evolution::{AggregationMethod, TemporalEntity},
    },
};

/// Pins: avg, min, max, sum and sumsq over the values 0.100000000000000005 and 0.200000000000000005 answer every
/// written digit, and stddev answers 0.070710678118654 followed by further digits
/// Clause: 4.5.19, 4.5.19.1, 5.7.3.4
/// Recorded: 4.5.19.1 does not fix the numeric precision of an aggregate; Andromeda aggregates the decimals as
/// written and answers them without rounding to binary64
#[gtest]
#[tokio::test]
async fn o_tmp_agg_0006_decimal_aggregates_keep_digits_beyond_binary64() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property(
            "reading",
            Property::builder(Value::Number("0.100000000000000005".parse()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            "reading",
            Property::builder(Value::Number("0.200000000000000005".parse()?)).observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .aggregation_methods("avg,min,max,sum,sumsq,stddev".parse()?)
        .send()
        .await?;
    let aggregated = retrieved.body();
    for (method, digits) in [
        (AggregationMethod::Avg, "0.150000000000000005"),
        (AggregationMethod::Min, "0.100000000000000005"),
        (AggregationMethod::Max, "0.200000000000000005"),
        (AggregationMethod::Sum, "0.30000000000000001"),
        (AggregationMethod::Sumsq, "0.05000000000000000300000000000000005"),
    ] {
        expect_that!(aggregated, has_aggregate("reading", method, nth(0, period().digits(digits))));
    }
    expect_that!(
        aggregated,
        has_aggregate("reading", AggregationMethod::Stddev, nth(0, period().digits_starting("0.070710678118654")))
    );
    Ok(())
}
