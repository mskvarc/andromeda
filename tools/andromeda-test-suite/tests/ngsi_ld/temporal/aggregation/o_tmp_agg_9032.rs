//! Modifying the string instance into a number makes sum eligible and back.

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
        Attribute,
        AttributeName,
        ErrorType,
        InstanceId,
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: a reading holding a number and a string instance refuses sum with 400 `InvalidRequest`, answers the sum of
/// both numbers once the string instance is modified into a number, and refuses sum again once it is modified back
/// into a string
/// Clause: 5.7.4.4, 5.6.14.4, Table 4.5.19.1-1
/// Recorded: Tables 4.5.19.1-1 to 4.5.19.1-3 mark a method N/A for a value datatype and do not say how N/A is
/// answered; Andromeda refuses a request whose attrs names an attribute holding an instance of that datatype with
/// 400 `InvalidRequest`, judged over the instances that exist
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9032_modifying_the_string_instance_into_a_number_makes_sum_eligible_and_back() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let aggregation_subject_modified_id = namespace.urn("AggregationSubject", "modified")?;
    let aggregation_subject_type = namespace.entity_type("AggregationSubject")?;
    let aggregation_subject_modified_evolution = TemporalEntity::builder()
        .id(aggregation_subject_modified_id.clone())
        .entity_type(aggregation_subject_type)
        .property(
            namespace.attribute("reading")?,
            Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .identified(
            namespace.attribute("reading")?,
            format!("urn:ngsi-ld:Instance:{namespace}-changing").parse::<InstanceId>()?,
            Property::builder("high").observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .build()?;
    broker
        .temporal()
        .create_or_update(&aggregation_subject_modified_evolution)
        .send()
        .await?
        .created()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_modified_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Sum]))
        .attrs(vec![format!("reading_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let instance_id = format!("urn:ngsi-ld:Instance:{namespace}-changing").parse::<InstanceId>()?;
    let instance = Attribute::from(Property::builder(2).observed_at("2024-01-01T01:00:00Z".parse()?).build()?);
    broker
        .temporal()
        .modify_instance(
            &aggregation_subject_modified_id,
            &format!("reading_{namespace}").parse::<AttributeName>()?,
            &instance_id,
            &instance,
        )
        .send()
        .await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_modified_id)
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
            all!(len(eq(1)), nth(0, period().value(3)))
        )
    );
    let instance_2 = Attribute::from(Property::builder("low").observed_at("2024-01-01T01:00:00Z".parse()?).build()?);
    broker
        .temporal()
        .modify_instance(
            &aggregation_subject_modified_id,
            &format!("reading_{namespace}").parse::<AttributeName>()?,
            &instance_id,
            &instance_2,
        )
        .send()
        .await?;
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_modified_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Sum]))
        .attrs(vec![format!("reading_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
