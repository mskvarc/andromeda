//! A relationship admits only the counting methods.

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
        Relationship,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: avg, min, max, stddev, sum or sumsq over a Relationship named in attrs answers 400 `InvalidRequest`, and
/// `distinctCount` and `totalCount` over two targets answer 2
/// Clause: Table 4.5.19.1-3, 5.7.3.4
/// Recorded: Tables 4.5.19.1-1 to 4.5.19.1-3 mark a method N/A for a value datatype and do not say how N/A is
/// answered; Andromeda refuses a request whose attrs names an attribute of that datatype with 400 `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9007_a_relationship_admits_only_the_counting_methods() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let aggregation_subject_relationship_id = namespace.urn("AggregationSubject", "relationship")?;
    let aggregation_subject_type = namespace.entity_type("AggregationSubject")?;
    let off_street_parking_id = namespace.urn("OffStreetParking", 1)?;
    let off_street_parking_2_id = namespace.urn("OffStreetParking", 2)?;
    let aggregation_subject_relationship_evolution = TemporalEntity::builder()
        .id(aggregation_subject_relationship_id.clone())
        .entity_type(aggregation_subject_type)
        .relationship(
            namespace.attribute("parkedAt")?,
            Relationship::builder(off_street_parking_id).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .relationship(
            namespace.attribute("parkedAt")?,
            Relationship::builder(off_street_parking_2_id).observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .build()?;
    broker
        .temporal()
        .create_or_update(&aggregation_subject_relationship_evolution)
        .send()
        .await?
        .created()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_relationship_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Avg]))
        .attrs(vec![format!("parkedAt_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_relationship_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Min]))
        .attrs(vec![format!("parkedAt_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_relationship_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Max]))
        .attrs(vec![format!("parkedAt_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_relationship_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Stddev]))
        .attrs(vec![format!("parkedAt_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_relationship_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Sum]))
        .attrs(vec![format!("parkedAt_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_relationship_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Sumsq]))
        .attrs(vec![format!("parkedAt_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_relationship_id)
        .temporal_q(
            temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::DistinctCount, AggregationMethod::TotalCount]),
        )
        .attrs(vec![format!("parkedAt_{namespace}").parse()?])
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(
            &format!("parkedAt_{namespace}"),
            AggregationMethod::DistinctCount,
            all!(len(eq(1)), nth(0, period().value(2)))
        )
    );
    expect_that!(
        aggregated,
        has_aggregate(
            &format!("parkedAt_{namespace}"),
            AggregationMethod::TotalCount,
            all!(len(eq(1)), nth(0, period().value(2)))
        )
    );
    Ok(())
}
