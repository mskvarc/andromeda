//! Numbers strings and relationships aggregate each by its own table.

use andromeda_test_suite::{
    aggregated_answer::{retrieve_unread, unbounded},
    body_json::json_equals,
    broker,
    json_scalar::json_number_equals,
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, Relationship, evolution::TemporalEntity},
    query::temporal_query::AggregationMethods,
};
use serde_json::Value;

/// Pins: avg, min, max, `totalCount` and `distinctCount` over a number, a string and a Relationship answer per
/// type: 150, 100, 200, 2, 2 for the number; null, closed, open, 2, 2 for the string; null, 2 and 2 for the
/// Relationship, which stays a Relationship
/// Clause: 4.5.19, 4.5.19.1, 5.7.3.4
/// Recorded: Table 4.5.19.1-1 marks the method N/A for this value type and does not say how N/A is answered;
/// Andromeda answers the triple with a null value
#[gtest]
#[tokio::test]
async fn o_tmp_agg_0041_numbers_strings_and_relationships_aggregate_each_by_its_own_table() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(100).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(200).observed_at("2024-01-01T01:00:00Z".parse()?))
        .property("status", Property::builder("open").observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("status", Property::builder("closed").observed_at("2024-01-01T01:00:00Z".parse()?))
        .relationship(
            "isParked",
            Relationship::builder(namespace.urn("Parking", "P1")?).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .relationship(
            "isParked",
            Relationship::builder(namespace.urn("Parking", "P2")?).observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    // A null aggregate sits beside the numbers and strings of other attributes, which the typed aggregated reader
    // cannot hold, so the typed request is answered as received.
    let methods = "avg,min,max,totalCount,distinctCount".parse::<AggregationMethods>()?;
    let aggregated = retrieve_unread(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), &agg_id, unbounded(&methods), &[]).await?;
    expect_that!(aggregated.status(), eq(StatusCode::OK));
    let body = aggregated.body();
    for (method, expected) in [("avg", 150.0), ("min", 100.0), ("max", 200.0), ("totalCount", 2.0), ("distinctCount", 2.0)] {
        expect_that!(body, json_number_equals(&format!("$.speed.{method}[0][0]"), expected));
    }
    expect_that!(body, json_equals("$.status.avg[0][0]", Value::Null));
    expect_that!(body, json_equals("$.status.min[0][0]", "closed"));
    expect_that!(body, json_equals("$.status.max[0][0]", "open"));
    expect_that!(body, json_number_equals("$.status.totalCount[0][0]", 2.0));
    expect_that!(body, json_number_equals("$.status.distinctCount[0][0]", 2.0));
    expect_that!(body, json_equals("$.isParked.avg[0][0]", Value::Null));
    expect_that!(body, json_number_equals("$.isParked.totalCount[0][0]", 2.0));
    expect_that!(body, json_number_equals("$.isParked.distinctCount[0][0]", 2.0));
    expect_that!(body, json_equals("$.isParked.type", "Relationship"));
    Ok(())
}
