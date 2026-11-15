//! A retrieval with a datasetId reaches only the sources registered for it or for no dataset.

use andromeda_test_suite::{
    broker,
    dataset_sources::{DatasetBinding, DatasetBoundSources},
    identity_matcher::has_id,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Property},
    registration::{DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: over inclusive registrations of one Entity listing the `datasetId`s [A] (source a) and [B] (source b), and
/// one listing none (source n), Retrieve Entity with `datasetId` A is forwarded to a and n, and without `datasetId`
/// to all three
/// Clause: 5.12, 5.7.1.4, 4.5.5
#[gtest]
#[tokio::test]
async fn n_fed_ret_9001_a_retrieval_with_a_dataset_id_reaches_only_the_sources_registered_for_it_or_for_no_dataset() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let dataset_a = namespace.dataset_id("Dataset", "a")?;
    let sources = DatasetBoundSources::register(
        &broker,
        &namespace,
        &RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?,
        &[DistributedOperation::RetrieveEntity],
        vec![
            DatasetBinding {
                prefix: "a",
                dataset_ids: Some(vec![dataset_a.clone()]),
            },
            DatasetBinding {
                prefix: "b",
                dataset_ids: Some(vec![namespace.dataset_id("Dataset", "b")?]),
            },
            DatasetBinding {
                prefix: "n",
                dataset_ids: None,
            },
        ],
    )
    .await?;
    let retrieved_at = |prefix: &str| SourceRequest::matching(Method::GET, &format!("/{prefix}/ngsi-ld/v1/entities/[^/]+"));
    let source_entity = Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).dataset_id(dataset_a.clone()))
        .build()?;
    sources.answer(retrieved_at, |_| source_answer::entity(&source_entity)).await?;
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .dataset_ids(vec![dataset_a.clone()])
        .send()
        .await?;
    expect_that!(retrieved.body(), has_id(&vehicle_id));
    let reached = sources.reached(|prefix| retrieved_at(prefix).query("datasetId", &dataset_a)).await?;
    expect_that!(reached, eq(&vec!["a", "n"]));
    let retrieved_2 = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    expect_that!(retrieved_2.body(), has_id(&vehicle_id));
    let reached_2 = sources.reached(|prefix| retrieved_at(prefix).without_query("datasetId")).await?;
    expect_that!(reached_2, eq(&vec!["a", "b", "n"]));
    Ok(())
}
