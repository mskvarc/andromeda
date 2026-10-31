//! Deleting the last string instance makes sum eligible again.

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
        AttributeName,
        ErrorType,
        InstanceId,
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: a reading holding a number and two string instances refuses sum with 400 `InvalidRequest`, still refuses
/// it once one string instance is deleted, and answers the sum of the number once the last string instance is
/// deleted
/// Clause: 5.7.4.4, 5.6.15.4, Table 4.5.19.1-1
/// Recorded: Tables 4.5.19.1-1 to 4.5.19.1-3 mark a method N/A for a value datatype and do not say how N/A is
/// answered; Andromeda refuses a request whose attrs names an attribute holding an instance of that datatype with
/// 400 `InvalidRequest`, judged over the instances that exist
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9031_deleting_the_last_string_instance_makes_sum_eligible_again() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let aggregation_subject_deleted_id = namespace.urn("AggregationSubject", "deleted")?;
    let aggregation_subject_type = namespace.entity_type("AggregationSubject")?;
    let aggregation_subject_deleted_evolution = TemporalEntity::builder()
        .id(aggregation_subject_deleted_id.clone())
        .entity_type(aggregation_subject_type)
        .property(
            namespace.attribute("reading")?,
            Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .identified(
            namespace.attribute("reading")?,
            format!("urn:ngsi-ld:Instance:{namespace}-high").parse::<InstanceId>()?,
            Property::builder("high").observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .identified(
            namespace.attribute("reading")?,
            format!("urn:ngsi-ld:Instance:{namespace}-low").parse::<InstanceId>()?,
            Property::builder("low").observed_at("2024-01-01T02:00:00Z".parse()?),
        )
        .build()?;
    broker
        .temporal()
        .create_or_update(&aggregation_subject_deleted_evolution)
        .send()
        .await?
        .created()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_deleted_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Sum]))
        .attrs(vec![format!("reading_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let instance_id = format!("urn:ngsi-ld:Instance:{namespace}-high").parse::<InstanceId>()?;
    broker
        .temporal()
        .delete_instance(
            &aggregation_subject_deleted_id,
            &format!("reading_{namespace}").parse::<AttributeName>()?,
            &instance_id,
        )
        .send()
        .await?;
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_deleted_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Sum]))
        .attrs(vec![format!("reading_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let instance_id_2 = format!("urn:ngsi-ld:Instance:{namespace}-low").parse::<InstanceId>()?;
    broker
        .temporal()
        .delete_instance(
            &aggregation_subject_deleted_id,
            &format!("reading_{namespace}").parse::<AttributeName>()?,
            &instance_id_2,
        )
        .send()
        .await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_deleted_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Sum]))
        .attrs(vec![format!("reading_{namespace}").parse()?])
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(
            &format!("reading_{namespace}"),
            AggregationMethod::Sum,
            all!(len(eq(1)), nth(0, period().value(1)))
        )
    );
    Ok(())
}
