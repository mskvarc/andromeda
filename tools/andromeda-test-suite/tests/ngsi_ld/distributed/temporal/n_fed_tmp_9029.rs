//! A temporal query and retrieval with a datasetId reach only the sources registered for it or for no dataset.

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
    model::{Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: over inclusive registrations of `Vehicle` Entities listing the `datasetId`s [A] (source a) and [B] (source
/// b), and one listing none (source n), Query Temporal Evolution of Entities by type and Retrieve Temporal Evolution
/// of an Entity, each with `datasetId` A, are forwarded to a and n alone
/// Clause: 5.12, 5.7.3.4, 5.7.4.4, 4.5.5
#[gtest]
#[tokio::test]
async fn n_fed_tmp_9029_a_temporal_query_and_retrieval_with_a_dataset_id_reach_only_the_sources_registered_for_it_or_for_no_dataset() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let dataset_a = namespace.dataset_id("Dataset", "a")?;
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
    let queried = |prefix: &str| {
        SourceRequest::new(Method::GET, &format!("/{prefix}/ngsi-ld/v1/temporal/entities")).query_containing("type", format!("Vehicle_{namespace}"))
    };
    let retrieved_at = |prefix: &str| SourceRequest::matching(Method::GET, &format!("/{prefix}/ngsi-ld/v1/temporal/entities/[^/]+"));
    let source_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(50).observed_at("2020-06-01T00:00:00Z".parse()?).dataset_id(dataset_a.clone()),
        )
        .build()?;
    sources.answer(queried, |_| source_answer::temporal_entities(&[])).await?;
    sources.answer(retrieved_at, |_| source_answer::temporal_entity(&source_evolution)).await?;
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .dataset_ids(vec![dataset_a.clone()])
        .send()
        .await?;
    expect_that!(page.items(), is_empty());
    let queried_reached = sources.reached(|prefix| queried(prefix).query("datasetId", &dataset_a)).await?;
    expect_that!(queried_reached, eq(&vec!["a", "n"]));
    let retrieved = broker
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .dataset_ids(vec![dataset_a.clone()])
        .send()
        .await?;
    expect_that!(&retrieved.body().id, eq(&vehicle_id));
    let retrieved_reached = sources.reached(|prefix| retrieved_at(prefix).query("datasetId", &dataset_a)).await?;
    expect_that!(retrieved_reached, eq(&vec!["a", "n"]));
    Ok(())
}
