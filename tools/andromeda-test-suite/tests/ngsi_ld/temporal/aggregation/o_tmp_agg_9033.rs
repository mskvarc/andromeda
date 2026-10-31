//! Replacing the string instance under its instance id makes sum eligible.

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
        InstanceId,
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: a reading holding a number and a string instance with a client `instanceId` refuses sum with 400
/// `InvalidRequest`, and answers the sum of both numbers once Append Attributes writes a number under the same
/// `instanceId`, which replaces the string instance
/// Clause: 5.7.4.4, 4.5.7, 5.6.12.4, Table 4.5.19.1-1
/// Recorded: Tables 4.5.19.1-1 to 4.5.19.1-3 mark a method N/A for a value datatype and do not say how N/A is
/// answered; Andromeda refuses a request whose attrs names an attribute holding an instance of that datatype with
/// 400 `InvalidRequest`, judged over the instances that exist; an instance written under a client `instanceId` the
/// attribute already holds replaces that instance
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9033_replacing_the_string_instance_under_its_instance_id_makes_sum_eligible() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let aggregation_subject_replaced_id = namespace.urn("AggregationSubject", "replaced")?;
    let aggregation_subject_type = namespace.entity_type("AggregationSubject")?;
    let aggregation_subject_replaced_evolution = TemporalEntity::builder()
        .id(aggregation_subject_replaced_id.clone())
        .entity_type(aggregation_subject_type.clone())
        .property(
            namespace.attribute("reading")?,
            Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .identified(
            namespace.attribute("reading")?,
            format!("urn:ngsi-ld:Instance:{namespace}-replaced").parse::<InstanceId>()?,
            Property::builder("high").observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .build()?;
    broker
        .temporal()
        .create_or_update(&aggregation_subject_replaced_evolution)
        .send()
        .await?
        .created()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_replaced_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Sum]))
        .attrs(vec![format!("reading_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let instances = TemporalEntity::builder()
        .id(aggregation_subject_replaced_id.clone())
        .entity_type(aggregation_subject_type.clone())
        .identified(
            namespace.attribute("reading")?,
            format!("urn:ngsi-ld:Instance:{namespace}-replaced").parse::<InstanceId>()?,
            Property::builder(2).observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .build()?
        .attributes;
    broker.temporal().add_attributes(&aggregation_subject_replaced_id, &instances).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_replaced_id)
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
    Ok(())
}
