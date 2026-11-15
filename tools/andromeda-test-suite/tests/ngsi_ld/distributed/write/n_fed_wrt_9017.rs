//! A delete attribute with a datasetId reaches only the sources registered for it or for no dataset.

use andromeda_test_suite::{
    broker,
    dataset_sources::{DatasetBinding, DatasetBoundSources},
    namespace::Namespace,
    outcome::Done,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::operation::InstanceSelection,
    model::{AttributeName, Entity, Property},
    registration::{DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: over inclusive registrations of one Entity listing the `datasetId`s [A] (source a) and [B] (source b), and
/// one listing none (source n), Delete Attribute speed with `datasetId` A answers 204 and is forwarded to a and n
/// alone, and Delete Attribute speed without `datasetId` answers 204 and is forwarded to all three
/// Clause: 5.12, 5.6.5.4, 4.5.5
/// Recorded: clause 5.12 compares the `datasetId` parameter of any request that triggers the matching, and Delete
/// Attribute is one that names one
#[gtest]
#[tokio::test]
async fn n_fed_wrt_9017_a_delete_attribute_with_a_dataset_id_reaches_only_the_sources_registered_for_it_or_for_no_dataset() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let dataset_a = namespace.dataset_id("Dataset", "a")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).dataset_id(dataset_a.clone()))
        .property("speed", 60)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let sources = DatasetBoundSources::register(
        &broker,
        &namespace,
        &RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?,
        &[DistributedOperation::DeleteAttrs],
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
    let deleted_at = |prefix: &str| SourceRequest::matching(Method::DELETE, &format!("/{prefix}/ngsi-ld/v1/entities/[^/]+/attrs/speed"));
    sources.answer(deleted_at, |_| source_answer::status(StatusCode::NO_CONTENT)).await?;
    let speed: AttributeName = "speed".parse()?;
    broker
        .entities()
        .delete_attribute(&vehicle_id, &speed)
        .instances(InstanceSelection::Dataset(&dataset_a))
        .send()
        .await?
        .done()?;
    let reached = sources.reached(|prefix| deleted_at(prefix).query("datasetId", &dataset_a)).await?;
    expect_that!(reached, eq(&vec!["a", "n"]));
    broker.entities().delete_attribute(&vehicle_id, &speed).send().await?.done()?;
    let reached_2 = sources.reached(|prefix| deleted_at(prefix).without_query("datasetId")).await?;
    expect_that!(reached_2, eq(&vec!["a", "b", "n"]));
    Ok(())
}
