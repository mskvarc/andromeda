//! A forwarded entity map creation names only the datasets its registration lists.

use andromeda_test_suite::{
    broker,
    dataset_sources::{DatasetBinding, DatasetBoundSources},
    header_matcher::present,
    namespace::Namespace,
    received_status::ExpectedStatus,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::DatasetId,
    registration::{DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: over inclusive `createEntityMapQueryEntity` registrations of `Vehicle` Entities listing the `datasetId`s [A]
/// (source a) and [@none, A] (source d), and one listing none (source n), each source declining to create a map,
/// Create `EntityMap` for Query Entities by type and an Entity identifier answers 201 and asks a and d for A and n for
/// A and B when the client names A and B, and a for A, d for @none and A, and n for no `datasetId` when the client
/// names none
/// Clause: 4.3.6.1, 5.12, 5.14.4.4, 4.5.5
#[gtest]
#[tokio::test]
async fn n_map_qen_9017_a_forwarded_entity_map_creation_names_only_the_datasets_its_registration_lists() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let dataset_a = namespace.dataset_id("Dataset", "a")?;
    let dataset_b = namespace.dataset_id("Dataset", "b")?;
    let sources = DatasetBoundSources::register(
        &broker,
        &namespace,
        &RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?,
        &[DistributedOperation::CreateEntityMapQueryEntity],
        vec![
            DatasetBinding {
                prefix: "a",
                dataset_ids: Some(vec![dataset_a.clone()]),
            },
            DatasetBinding {
                prefix: "d",
                dataset_ids: Some(vec![DatasetId::DefaultInstance, dataset_a.clone()]),
            },
            DatasetBinding {
                prefix: "n",
                dataset_ids: None,
            },
        ],
    )
    .await?;
    let created_at = |prefix: &str| SourceRequest::new(Method::GET, &format!("/{prefix}/ngsi-ld/v1/entityMaps"));
    sources.answer(created_at, |_| source_answer::status(StatusCode::NOT_FOUND)).await?;
    for (vehicle, dataset_ids, expected) in [
        (
            1,
            vec![dataset_a.clone(), dataset_b.clone()],
            vec![
                ("a", Some(vec![dataset_a.clone()])),
                ("d", Some(vec![dataset_a.clone()])),
                ("n", Some(vec![dataset_a.clone(), dataset_b.clone()])),
            ],
        ),
        (
            2,
            Vec::new(),
            vec![
                ("a", Some(vec![dataset_a.clone()])),
                ("d", Some(vec![DatasetId::DefaultInstance, dataset_a.clone()])),
                ("n", None),
            ],
        ),
    ] {
        let vehicle_id = namespace.urn("Vehicle", vehicle)?;
        let created = broker
            .with_heads()
            .entity_maps()
            .create()
            .entity_type(&vehicle_type)
            .ids(vec![vehicle_id.clone()])
            .dataset_ids(dataset_ids.clone())
            .send()
            .await?
            .expecting(StatusCode::CREATED)?;
        expect_that!(created.entity_map(), present());
        let asked = sources.asked_in_query(|prefix| created_at(prefix).query("id", &vehicle_id)).await?;
        expect_that!(asked, eq(&expected), "datasetId {dataset_ids:?}");
        broker.entity_maps().delete(&created.body().id).send().await?;
    }
    Ok(())
}
