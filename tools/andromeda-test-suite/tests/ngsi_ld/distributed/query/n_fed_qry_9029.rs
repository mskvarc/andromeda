//! A forwarded batch query names only the datasets its registration lists.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
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

/// Pins: over inclusive `queryBatch` registrations of `Vehicle` Entities listing the `datasetId`s [@none, A] (source
/// d), and one listing none (source n), Query Entities by type and an Entity identifier is posted to d with the
/// `datasetId` A and to n with A and B when the client names A and B, and to d with @none and A and to n with no
/// `datasetId` when the client names none
/// Clause: 4.3.6.1, 5.12, 5.2.23, 6.23.3.1
/// Recorded: Table 5.2.23-1 gives the posted Query a `datasetId` member, which carries what a forwarded query string
/// would
#[gtest]
#[tokio::test]
async fn n_fed_qry_9029_a_forwarded_batch_query_names_only_the_datasets_its_registration_lists() -> Result<()> {
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
        &[DistributedOperation::QueryBatch],
        vec![
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
    let posted = |prefix: &str| SourceRequest::new(Method::POST, &format!("/{prefix}/ngsi-ld/v1/entityOperations/query"));
    sources.answer(posted, |_| source_answer::entities(&[])).await?;
    for (vehicle, dataset_ids, expected) in [
        (
            1,
            Some(vec![dataset_a.clone(), dataset_b.clone()]),
            vec![("d", Some(vec![dataset_a.clone()])), ("n", Some(vec![dataset_a.clone(), dataset_b.clone()]))],
        ),
        (2, None, vec![("d", Some(vec![DatasetId::DefaultInstance, dataset_a.clone()])), ("n", None)]),
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
        let asked = sources
            .asked_in_body(|prefix| posted(prefix).body(BodyPattern::Contains(vehicle_id.to_string())))
            .await?;
        expect_that!(asked, eq(&expected), "datasetId {dataset_ids:?}");
    }
    Ok(())
}
