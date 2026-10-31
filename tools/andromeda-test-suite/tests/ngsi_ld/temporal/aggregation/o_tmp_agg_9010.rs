//! A list relationship and a vocab property admit counting alone.

use andromeda_test_suite::{
    aggregated_answer::retrieve_unread,
    body_json::{json_count, json_equals},
    broker,
    json_scalar::json_number_equals,
    namespace::Namespace,
    outcome::Upserted,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::TemporalSelection},
    model::{
        ErrorType,
        ListRelationship,
        NgsiLdMediaType,
        VocabProperty,
        VocabTerm,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query::{self, AggregationMethods},
};
use serde_json::Value;

/// Pins: avg, min, max, stddev, sum or sumsq over a `ListRelationship` or a multi-member `VocabProperty` named in
/// attrs answers 400 `InvalidRequest` and `totalCount` answers 2; without attrs every arithmetic method answers a
/// null value for both
/// Clause: Table 4.5.19.1-2, Table 4.5.19.1-3, 4.5.20.2, 4.5.22.1, 5.7.3.4
/// Recorded: Tables 4.5.19.1-1 to 4.5.19.1-3 mark a method N/A for a value datatype and do not say how N/A is
/// answered; Andromeda refuses a request whose attrs names an attribute of that datatype with 400 `InvalidRequest`
/// Recorded: the two lists hold 2 and 4 members, so an aggregation by list size would answer avg 3.
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9010_a_list_relationship_and_a_vocab_property_admit_counting_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let subject_id = namespace.urn("AggregationSubject", "relationship-family")?;
    let subject_type = namespace.entity_type("AggregationSubject")?;
    let route = namespace.suffixed("route");
    let categories = namespace.suffixed("categories");
    let term = |text: &str| text.parse::<VocabTerm>();
    let subject_evolution = TemporalEntity::builder()
        .id(subject_id.clone())
        .entity_type(subject_type)
        .list_relationship(
            namespace.attribute("route")?,
            ListRelationship::builder(vec![namespace.urn("City", "antwerp")?, namespace.urn("City", "rotterdam")?])
                .observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .list_relationship(
            namespace.attribute("route")?,
            ListRelationship::builder(vec![
                namespace.urn("City", "antwerp")?,
                namespace.urn("City", "rotterdam")?,
                namespace.urn("City", "amsterdam")?,
                namespace.urn("City", "ghent")?,
            ])
            .observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .vocab_property(
            namespace.attribute("categories")?,
            VocabProperty::builder(vec![term("alpha")?, term("beta")?]).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .vocab_property(
            namespace.attribute("categories")?,
            VocabProperty::builder(vec![term("alpha")?, term("beta")?, term("gamma")?, term("delta")?]).observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&subject_evolution).send().await?.created()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for attribute in [&route, &categories] {
        for method in [
            AggregationMethod::Avg,
            AggregationMethod::Min,
            AggregationMethod::Max,
            AggregationMethod::Stddev,
            AggregationMethod::Sum,
            AggregationMethod::Sumsq,
        ] {
            let refused = json
                .with_responses()
                .temporal()
                .retrieve_aggregated(&subject_id)
                .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([method]))
                .attrs(vec![attribute.parse()?])
                .send()
                .await;
            expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
        }
        // A ListRelationship or a VocabProperty is answered under its own type, which the typed aggregated reader
        // refuses, so the typed request is answered as received.
        let counting = temporal_query::after("2023-12-31T00:00:00Z".parse()?)
            .aggregation_methods([AggregationMethod::DistinctCount, AggregationMethod::TotalCount])
            .build()?;
        let counted = retrieve_unread(&json, &subject_id, TemporalSelection::Query(&counting), &[attribute.parse()?]).await?;
        expect_that!(counted.status(), eq(StatusCode::OK));
        expect_that!(counted.body(), json_count(&format!("$.{attribute}.totalCount"), 1));
        expect_that!(counted.body(), json_number_equals(&format!("$.{attribute}.totalCount[0][0]"), 2.0));
    }
    // The null aggregates the arithmetic methods answer without attrs are held by no typed reader, so the typed
    // request is answered as received.
    let methods = "avg,min,max,stddev,sum,sumsq".parse::<AggregationMethods>()?;
    let window = temporal_query::after("2023-12-31T00:00:00Z".parse()?)
        .aggregation_methods(methods.as_slice().iter().copied())
        .build()?;
    let unrestricted = retrieve_unread(&json, &subject_id, TemporalSelection::Query(&window), &[]).await?;
    expect_that!(unrestricted.status(), eq(StatusCode::OK));
    for attribute in [&route, &categories] {
        for method in methods.as_slice() {
            expect_that!(unrestricted.body(), json_count(&format!("$.{attribute}.{method}"), 1));
            expect_that!(unrestricted.body(), json_equals(&format!("$.{attribute}.{method}[0][0]"), Value::Null));
        }
    }
    Ok(())
}
