//! Deleting the dataset holding strings makes sum eligible again.

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
    http::{headers::Accept, operation::InstanceSelection},
    model::{
        AttributeName,
        ErrorType,
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: a reading holding numbers in its default instance and a string in a `datasetId` instance refuses sum with
/// 400 `InvalidRequest`, and answers the sum of the numbers once Delete Attribute removes that `datasetId`
/// Clause: 5.7.4.4, 5.6.13.4, Table 4.5.19.1-1
/// Recorded: Tables 4.5.19.1-1 to 4.5.19.1-3 mark a method N/A for a value datatype and do not say how N/A is
/// answered; Andromeda refuses a request whose attrs names an attribute holding an instance of that datatype with
/// 400 `InvalidRequest`, judged over the instances that exist
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9034_deleting_the_dataset_holding_strings_makes_sum_eligible_again() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let aggregation_subject_dataset_id = namespace.urn("AggregationSubject", "dataset")?;
    let aggregation_subject_type = namespace.entity_type("AggregationSubject")?;
    let text_dataset = namespace.dataset_id("Dataset", "text")?;
    let aggregation_subject_dataset_evolution = TemporalEntity::builder()
        .id(aggregation_subject_dataset_id.clone())
        .entity_type(aggregation_subject_type)
        .property(
            namespace.attribute("reading")?,
            Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            namespace.attribute("reading")?,
            Property::builder(2).observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .property(
            namespace.attribute("reading")?,
            Property::builder("high")
                .dataset_id(namespace.dataset_id("Dataset", "text")?)
                .observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .build()?;
    broker
        .temporal()
        .create_or_update(&aggregation_subject_dataset_evolution)
        .send()
        .await?
        .created()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_dataset_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Sum]))
        .attrs(vec![format!("reading_{namespace}").parse()?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    broker
        .temporal()
        .delete_attribute(&aggregation_subject_dataset_id, &format!("reading_{namespace}").parse::<AttributeName>()?)
        .instances(InstanceSelection::Dataset(&text_dataset))
        .send()
        .await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_dataset_id)
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
