//! A temporal delete attribute with a datasetId reaches only the sources registered for it or for no dataset.

use andromeda_test_suite::{
    broker,
    dataset_sources::{DatasetBinding, DatasetBoundSources},
    namespace::Namespace,
    outcome::Upserted,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::operation::InstanceSelection,
    model::{AttributeName, Property, evolution::TemporalEntity},
    registration::{DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: over inclusive registrations of one Temporal Evolution listing the `datasetId`s [A] (source a) and [B]
/// (source b), and one listing none (source n), Delete Attribute from Temporal Evolution of an Entity speed with
/// `datasetId` A is forwarded to a and n alone
/// Clause: 5.12, 5.6.13.4, 4.5.5
#[gtest]
#[tokio::test]
async fn n_fed_wrt_9018_a_temporal_delete_attribute_with_a_dataset_id_reaches_only_the_sources_registered_for_it_or_for_no_dataset() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let dataset_a = namespace.dataset_id("Dataset", "a")?;
    let evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(50).observed_at("2020-06-01T00:00:00Z".parse()?).dataset_id(dataset_a.clone()),
        )
        .property("speed", Property::builder(60).observed_at("2020-06-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&evolution).send().await?.created()?;
    let sources = DatasetBoundSources::register(
        &broker,
        &namespace,
        &RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?,
        &[DistributedOperation::DeleteAttrsTemporal],
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
    let deleted_at = |prefix: &str| SourceRequest::matching(Method::DELETE, &format!("/{prefix}/ngsi-ld/v1/temporal/entities/[^/]+/attrs/speed"));
    sources.answer(deleted_at, |_| source_answer::status(StatusCode::NO_CONTENT)).await?;
    broker
        .temporal()
        .delete_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?)
        .instances(InstanceSelection::Dataset(&dataset_a))
        .send()
        .await?;
    let reached = sources.reached(|prefix| deleted_at(prefix).query("datasetId", &dataset_a)).await?;
    expect_that!(reached, eq(&vec!["a", "n"]));
    Ok(())
}
