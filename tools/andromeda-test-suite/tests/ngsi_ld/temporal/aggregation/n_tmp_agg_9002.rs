//! Min max and sum over json arrays measure array size.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    value::object,
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

/// Pins: min, max and sum over a JSON Array Property observed as arrays of 2 and of 4 elements answer 2, 4 and 6
/// Clause: Table 4.5.19.1-1, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_agg_9002_min_max_and_sum_over_json_arrays_measure_array_size() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let aggregation_subject_array_extrema_id = namespace.urn("AggregationSubject", "array-extrema")?;
    let aggregation_subject_type = namespace.entity_type("AggregationSubject")?;
    let aggregation_subject_array_extrema_evolution = TemporalEntity::builder()
        .id(aggregation_subject_array_extrema_id.clone())
        .entity_type(aggregation_subject_type)
        .property(
            namespace.attribute("tags")?,
            Property::builder(object([
                ("@type", Value::from("@json")),
                ("@value", Value::Array(vec![Value::from(1), Value::from(2)])),
            ]))
            .observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            namespace.attribute("tags")?,
            Property::builder(object([
                ("@type", Value::from("@json")),
                ("@value", Value::Array(vec![Value::from(1), Value::from(2), Value::from(3), Value::from(4)])),
            ]))
            .observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .build()?;
    broker
        .temporal()
        .create_or_update(&aggregation_subject_array_extrema_evolution)
        .send()
        .await?
        .created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_array_extrema_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([
            AggregationMethod::Min,
            AggregationMethod::Max,
            AggregationMethod::Sum,
        ]))
        .attrs(vec![format!("tags_{namespace}").parse()?])
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(
            &format!("tags_{namespace}"),
            AggregationMethod::Min,
            all!(len(eq(1)), nth(0, period().value(2)))
        )
    );
    expect_that!(
        aggregated,
        has_aggregate(
            &format!("tags_{namespace}"),
            AggregationMethod::Max,
            all!(len(eq(1)), nth(0, period().value(4)))
        )
    );
    expect_that!(
        aggregated,
        has_aggregate(
            &format!("tags_{namespace}"),
            AggregationMethod::Sum,
            all!(len(eq(1)), nth(0, period().value(6)))
        )
    );
    Ok(())
}
