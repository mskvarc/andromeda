//! A forwarded retrieval names only the datasets its registration lists.

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
    model::{DatasetId, Entity, EntityId, Property},
    registration::{DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: over inclusive registrations of `Vehicle` Entities listing the `datasetId`s [A] (source a) and [@none, A]
/// (source d), and one listing none (source n), Retrieve Entity of one Entity with the `datasetId`s A and B asks a
/// and d for A and n for A and B, and of another with no `datasetId` asks a for A, d for @none and A, and n for none
/// Clause: 4.3.6.1, 5.12, 5.7.1.4, 4.5.5
#[gtest]
#[tokio::test]
async fn n_fed_ret_9002_a_forwarded_retrieval_names_only_the_datasets_its_registration_lists() -> Result<()> {
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
        &[DistributedOperation::RetrieveEntity],
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
    let retrieved_at = |prefix: &str, entity_id: &EntityId| SourceRequest::new(Method::GET, &format!("/{prefix}/ngsi-ld/v1/entities/{entity_id}"));
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
        let source_entity = Entity::output_builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(50).dataset_id(dataset_a.clone()))
            .build()?;
        sources
            .answer(|prefix| retrieved_at(prefix, &vehicle_id), |_| source_answer::entity(&source_entity))
            .await?;
        let client = broker.with_accept(Accept::any());
        let entities = client.entities();
        let retrieval = entities.retrieve(&vehicle_id);
        let retrieved = match dataset_ids.clone() {
            Some(dataset_ids) => retrieval.dataset_ids(dataset_ids).send().await?,
            None => retrieval.send().await?,
        };
        expect_that!(retrieved.body(), has_id(&vehicle_id));
        let asked = sources.asked_in_query(|prefix| retrieved_at(prefix, &vehicle_id)).await?;
        expect_that!(asked, eq(&expected), "datasetId {dataset_ids:?}");
    }
    Ok(())
}
