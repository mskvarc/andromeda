//! A string held only outside the window refuses no aggregation.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
    value::object,
};
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

/// Pins: a Property holding a String in January and JSON Arrays of 2 and 4 elements in February answers avg 3 for a
/// retrieval and a query whose window starts after the String, and refuses avg with 400 `InvalidRequest` when the
/// window holds the String
/// Clause: 5.7.4.4, 5.7.3.4, Table 4.5.19.1-1
/// Recorded: clause 5.7.4.4 raises `InvalidRequest` when a requested Attribute is not eligible right after limiting
/// the representation to the instances that meet the temporal restrictions, and does not say whether instances
/// outside the window count; Andromeda judges eligibility over the instances the representation aggregates
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9037_a_string_held_only_outside_the_window_refuses_no_aggregation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_relabelled_id = namespace.urn("Sensor", "relabelled")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_relabelled_evolution = TemporalEntity::builder()
        .id(sensor_relabelled_id.clone())
        .entity_type(sensor_type.clone())
        .property(
            namespace.attribute("level")?,
            Property::builder("unknown").observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            namespace.attribute("level")?,
            Property::builder(object([
                ("@type", Value::from("@json")),
                ("@value", Value::Array(vec![Value::from(1), Value::from(2)])),
            ]))
            .observed_at("2024-02-01T00:00:00Z".parse()?),
        )
        .property(
            namespace.attribute("level")?,
            Property::builder(object([
                ("@type", Value::from("@json")),
                ("@value", Value::Array(vec![Value::from(1), Value::from(2), Value::from(3), Value::from(4)])),
            ]))
            .observed_at("2024-02-02T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&sensor_relabelled_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&sensor_relabelled_id)
        .temporal_q(temporal_query::after("2024-01-15T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Avg]))
        .attrs(vec![format!("level_{namespace}").parse()?])
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(
            &format!("level_{namespace}"),
            AggregationMethod::Avg,
            all!(len(eq(1)), nth(0, period().value(3)))
        )
    );
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query_aggregated(temporal_query::after("2024-01-15T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Avg]))
        .entity_type(&sensor_type)
        .attrs(vec![format!("level_{namespace}").parse()?])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let aggregated_2 = page.items().first().or_fail()?;
    expect_that!(
        aggregated_2,
        has_aggregate(&format!("level_{namespace}"), AggregationMethod::Avg, nth(0, period().value(3)))
    );
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&sensor_relabelled_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Avg]))
        .attrs(vec![format!("level_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query_aggregated(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Avg]))
        .entity_type(&sensor_type)
        .attrs(vec![format!("level_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
