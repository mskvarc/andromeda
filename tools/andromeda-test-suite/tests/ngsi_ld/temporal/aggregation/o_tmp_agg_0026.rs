//! Sum of strings answers null.

use andromeda_test_suite::{
    aggregated_answer::{retrieve_unread, unbounded},
    body_json::{json_count, json_equals},
    broker,
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query::AggregationMethods,
};
use serde_json::Value;

/// Pins: sum over string instances, N/A for strings, answers one triple whose value is null
/// Clause: 4.5.19, 4.5.19.1, 5.7.3.4
/// Recorded: Table 4.5.19.1-1 marks the method N/A for this value type and does not say how N/A is answered;
/// Andromeda answers the triple with a null value
#[gtest]
#[tokio::test]
async fn o_tmp_agg_0026_sum_of_strings_answers_null() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("status", Property::builder("banana").observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("status", Property::builder("apple").observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("status", Property::builder("cherry").observed_at("2024-01-01T02:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    // The aggregate is a null, which the typed aggregated reader cannot hold, so the typed request is answered as received.
    let methods = AggregationMethods::new([AggregationMethod::Sum]).or_fail()?;
    let aggregated = retrieve_unread(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), &agg_id, unbounded(&methods), &[]).await?;
    expect_that!(aggregated.status(), eq(StatusCode::OK));
    expect_that!(aggregated.body(), json_count("$.status.sum", 1));
    expect_that!(aggregated.body(), json_equals("$.status.sum[0][0]", Value::Null));
    Ok(())
}
