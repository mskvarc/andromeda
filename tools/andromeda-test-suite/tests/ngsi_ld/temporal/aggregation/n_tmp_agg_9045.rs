//! The numeric aggregations count declared numbers and skip the values beyond the decimals.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    value::xsd_typed,
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
    query::temporal_query,
};

/// Pins: over one period holding the `reading` instances "1.5" and "2.5" declared xsd:decimal and "INF" declared
/// xsd:double, avg, min, max and sum answer 2, 1.5, 2.5 and 4: a declared number is a number, and INF has no JSON
/// number for the result to carry
/// Clause: Table 4.5.19.1-1, 4.5.19.1, 5.7.3.4, C.6
#[gtest]
#[tokio::test]
async fn n_tmp_agg_9045_the_numeric_aggregations_count_declared_numbers_and_skip_infinity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let reading = namespace.attribute("reading")?;
    let sensor_evolution = TemporalEntity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type)
        .property(
            reading.clone(),
            Property::builder(xsd_typed("decimal", "1.5")?).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            reading.clone(),
            Property::builder(xsd_typed("decimal", "2.5")?).observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .property(
            reading,
            Property::builder(xsd_typed("double", "INF")?).observed_at("2024-01-01T02:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&sensor_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&sensor_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([
            AggregationMethod::Avg,
            AggregationMethod::Min,
            AggregationMethod::Max,
            AggregationMethod::Sum,
        ]))
        .attrs(vec![format!("reading_{namespace}").parse()?])
        .send()
        .await?;
    let aggregated = retrieved.body();
    let attribute = format!("reading_{namespace}");
    expect_that!(
        aggregated,
        has_aggregate(&attribute, AggregationMethod::Avg, all!(len(eq(1)), nth(0, period().value(2))))
    );
    expect_that!(
        aggregated,
        has_aggregate(
            &attribute,
            AggregationMethod::Min,
            all!(len(eq(1)), nth(0, period().value(Value::Number("1.5".parse()?))))
        )
    );
    expect_that!(
        aggregated,
        has_aggregate(
            &attribute,
            AggregationMethod::Max,
            all!(len(eq(1)), nth(0, period().value(Value::Number("2.5".parse()?))))
        )
    );
    expect_that!(
        aggregated,
        has_aggregate(&attribute, AggregationMethod::Sum, all!(len(eq(1)), nth(0, period().value(4))))
    );
    Ok(())
}
