//! Stddev and sumsq over json arrays are refused.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::problem, value::object};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        ErrorType,
        NgsiLdMediaType,
        Property,
        Value,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: stddev or sumsq over a JSON Array Property named in attrs answers 400 `InvalidRequest`
/// Clause: Table 4.5.19.1-1, 5.7.3.4
/// Recorded: Tables 4.5.19.1-1 to 4.5.19.1-3 mark a method N/A for a value datatype and do not say how N/A is
/// answered; Andromeda refuses a request whose attrs names an attribute of that datatype with 400 `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9003_stddev_and_sumsq_over_json_arrays_are_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let aggregation_subject_array_na_id = namespace.urn("AggregationSubject", "array-na")?;
    let aggregation_subject_type = namespace.entity_type("AggregationSubject")?;
    let aggregation_subject_array_na_evolution = TemporalEntity::builder()
        .id(aggregation_subject_array_na_id.clone())
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
        .create_or_update(&aggregation_subject_array_na_evolution)
        .send()
        .await?
        .created()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_array_na_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Stddev]))
        .attrs(vec![format!("tags_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_array_na_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Sumsq]))
        .attrs(vec![format!("tags_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
