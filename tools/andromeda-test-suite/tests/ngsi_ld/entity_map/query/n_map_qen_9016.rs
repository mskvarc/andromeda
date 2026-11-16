//! An entity map creation names only the datasets its registration lists.

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
    http::{headers::Accept, operation::EntityMapRequest},
    model::DatasetId,
    registration::{DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: over inclusive registrations of `Vehicle` Entities listing the `datasetId`s [A] (source a) and [@none, A]
/// (source d), and one listing none (source n), Query Entities by type and an Entity identifier with `entityMap` true
/// answers 201 and asks a and d for A and n for A and B when the client names A and B, and a for A, d for @none and
/// A, and n for no `datasetId` when the client names none
/// Clause: 4.3.6.1, 4.3.6.7, 5.12, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_map_qen_9016_an_entity_map_creation_names_only_the_datasets_its_registration_lists() -> Result<()> {
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
        &[DistributedOperation::QueryEntity],
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
    let queried =
        |prefix: &str| SourceRequest::new(Method::GET, &format!("/{prefix}/ngsi-ld/v1/entities")).query_containing("type", format!("Vehicle_{namespace}"));
    sources.answer(queried, |_| source_answer::entities(&[])).await?;
    let map_request = EntityMapRequest::returned();
    for (vehicle, dataset_ids, expected) in [
        (
            1,
            Some(vec![dataset_a.clone(), dataset_b.clone()]),
            vec![
                ("a", Some(vec![dataset_a.clone()])),
                ("d", Some(vec![dataset_a.clone()])),
                ("n", Some(vec![dataset_a.clone(), dataset_b.clone()])),
            ],
        ),
        (
            2,
            None,
            vec![
                ("a", Some(vec![dataset_a.clone()])),
                ("d", Some(vec![DatasetId::DefaultInstance, dataset_a.clone()])),
                ("n", None),
            ],
        ),
    ] {
        let vehicle_id = namespace.urn("Vehicle", vehicle)?;
        let client = broker.with_heads().with_accept(Accept::any());
        let entities = client.entities();
        let query = entities
            .query()
            .entity_type(&vehicle_type)
            .ids(vec![vehicle_id.clone()])
            .entity_map(&map_request);
        let page = match dataset_ids.clone() {
            Some(dataset_ids) => query.dataset_ids(dataset_ids).send().await?,
            None => query.send().await?,
        }
        .expecting(StatusCode::CREATED)?;
        expect_that!(page.entity_map(), present());
        let asked = sources.asked_in_query(|prefix| queried(prefix).query("id", &vehicle_id)).await?;
        expect_that!(asked, eq(&expected), "datasetId {dataset_ids:?}");
    }
    Ok(())
}
