//! A forwarded temporal query and retrieval name only the datasets their registration lists.

use andromeda_test_suite::{
    broker,
    dataset_sources::{DatasetBinding, DatasetBoundSources},
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{DatasetId, EntityId, Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: over inclusive registrations of `Vehicle` Entities listing the `datasetId`s [A] (source a) and [@none, A]
/// (source d), and one listing none (source n), Query Temporal Evolution of Entities by type and an Entity identifier
/// and Retrieve Temporal Evolution of an Entity each ask a and d for A and n for A and B when the client names A and B,
/// and a for A, d for @none and A, and n for no `datasetId` when the client names none
/// Clause: 4.3.6.1, 5.12, 5.7.3.4, 5.7.4.4, 4.5.5
#[gtest]
#[tokio::test]
async fn n_fed_tmp_9030_a_forwarded_temporal_query_and_retrieval_name_only_the_datasets_their_registration_lists() -> Result<()> {
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
        &[DistributedOperation::QueryTemporal, DistributedOperation::RetrieveTemporal],
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
    let queried = |prefix: &str| {
        SourceRequest::new(Method::GET, &format!("/{prefix}/ngsi-ld/v1/temporal/entities")).query_containing("type", format!("Vehicle_{namespace}"))
    };
    let retrieved_at = |prefix: &str, entity_id: &EntityId| SourceRequest::new(Method::GET, &format!("/{prefix}/ngsi-ld/v1/temporal/entities/{entity_id}"));
    sources.answer(queried, |_| source_answer::temporal_entities(&[])).await?;
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
        let client = broker.with_accept(Accept::any());
        let temporal = client.temporal();
        let query = temporal
            .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
            .entity_type(&vehicle_type)
            .ids(vec![vehicle_id.clone()]);
        let page = match dataset_ids.clone() {
            Some(dataset_ids) => query.dataset_ids(dataset_ids).send().await?,
            None => query.send().await?,
        };
        expect_that!(page.items(), is_empty());
        let queried_asked = sources.asked_in_query(|prefix| queried(prefix).query("id", &vehicle_id)).await?;
        expect_that!(queried_asked, eq(&expected), "query with datasetId {dataset_ids:?}");
        let source_evolution = TemporalEntity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property(
                "speed",
                Property::builder(50).observed_at("2020-06-01T00:00:00Z".parse()?).dataset_id(dataset_a.clone()),
            )
            .build()?;
        sources
            .answer(
                |prefix| retrieved_at(prefix, &vehicle_id),
                |_| source_answer::temporal_entity(&source_evolution),
            )
            .await?;
        let retrieval = temporal
            .retrieve(&vehicle_id)
            .temporal_q(temporal_query::after("2020-01-01T00:00:00Z".parse()?));
        let retrieved = match dataset_ids.clone() {
            Some(dataset_ids) => retrieval.dataset_ids(dataset_ids).send().await?,
            None => retrieval.send().await?,
        };
        expect_that!(&retrieved.body().id, eq(&vehicle_id));
        let retrieved_asked = sources.asked_in_query(|prefix| retrieved_at(prefix, &vehicle_id)).await?;
        expect_that!(retrieved_asked, eq(&expected), "retrieval with datasetId {dataset_ids:?}");
    }
    Ok(())
}
