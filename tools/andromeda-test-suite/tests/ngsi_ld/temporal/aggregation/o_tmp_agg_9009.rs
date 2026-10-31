//! A json string admits ordering but no arithmetic.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        ErrorType,
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: avg, stddev, sum or sumsq over a String Property named in attrs answers 400 `InvalidRequest`, and min and
/// max over beta then alpha answer alpha and beta
/// Clause: Table 4.5.19.1-1, 5.7.3.4
/// Recorded: Tables 4.5.19.1-1 to 4.5.19.1-3 mark a method N/A for a value datatype and do not say how N/A is
/// answered; Andromeda refuses a request whose attrs names an attribute of that datatype with 400 `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9009_a_json_string_admits_ordering_but_no_arithmetic() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let aggregation_subject_string_id = namespace.urn("AggregationSubject", "string")?;
    let aggregation_subject_type = namespace.entity_type("AggregationSubject")?;
    let aggregation_subject_string_evolution = TemporalEntity::builder()
        .id(aggregation_subject_string_id.clone())
        .entity_type(aggregation_subject_type)
        .property(
            namespace.attribute("brandName")?,
            Property::builder("beta").observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            namespace.attribute("brandName")?,
            Property::builder("alpha").observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .build()?;
    broker
        .temporal()
        .create_or_update(&aggregation_subject_string_evolution)
        .send()
        .await?
        .created()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_string_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Avg]))
        .attrs(vec![format!("brandName_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_string_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Stddev]))
        .attrs(vec![format!("brandName_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_string_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Sum]))
        .attrs(vec![format!("brandName_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_string_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Sumsq]))
        .attrs(vec![format!("brandName_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_string_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Min, AggregationMethod::Max]))
        .attrs(vec![format!("brandName_{namespace}").parse()?])
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(
            &format!("brandName_{namespace}"),
            AggregationMethod::Min,
            all!(len(eq(1)), nth(0, period().value("alpha")))
        )
    );
    expect_that!(
        aggregated,
        has_aggregate(
            &format!("brandName_{namespace}"),
            AggregationMethod::Max,
            all!(len(eq(1)), nth(0, period().value("beta")))
        )
    );
    Ok(())
}
