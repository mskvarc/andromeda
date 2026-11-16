//! A query with a datasetId reaches only the sources registered for it or for no dataset.

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
    model::DatasetId,
    registration::{DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: over inclusive registrations of `Vehicle` Entities listing the `datasetId`s [A] (source a), [B] (source b)
/// and [@none, D] (source d), and one listing none (source n), Query Entities by type and an Entity identifier with
/// `datasetId` A is forwarded to a and n, with A and B to a, b and n, with @none to d and n, and without `datasetId` to
/// all four
/// Clause: 5.12, 5.7.2.4, 4.5.5
/// Recorded: clause 5.12 says a registration listing a `datasetId` "should only be considered matching, if both have
/// at least one value in common"; Andromeda applies it as a requirement, and `@none` is a value like any other
#[gtest]
#[tokio::test]
async fn n_fed_qry_9027_a_query_with_a_dataset_id_reaches_only_the_sources_registered_for_it_or_for_no_dataset() -> Result<()> {
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
                prefix: "b",
                dataset_ids: Some(vec![dataset_b.clone()]),
            },
            DatasetBinding {
                prefix: "d",
                dataset_ids: Some(vec![DatasetId::DefaultInstance, namespace.dataset_id("Dataset", "d")?]),
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
    for (vehicle, dataset_ids, expected) in [
        (1, Some(vec![dataset_a.clone()]), vec!["a", "n"]),
        (2, Some(vec![dataset_a.clone(), dataset_b.clone()]), vec!["a", "b", "n"]),
        (3, Some(vec![DatasetId::DefaultInstance]), vec!["d", "n"]),
        (4, None, vec!["a", "b", "d", "n"]),
    ] {
        let vehicle_id = namespace.urn("Vehicle", vehicle)?;
        let client = broker.with_accept(Accept::any());
        let entities = client.entities();
        let query = entities.query().entity_type(&vehicle_type).ids(vec![vehicle_id.clone()]);
        let page = match dataset_ids.clone() {
            Some(dataset_ids) => query.dataset_ids(dataset_ids).send().await?,
            None => query.send().await?,
        };
        expect_that!(page.items(), is_empty());
        let reached = sources.reached(|prefix| queried(prefix).query("id", &vehicle_id)).await?;
        expect_that!(reached, eq(&expected), "datasetId {dataset_ids:?}");
    }
    Ok(())
}
