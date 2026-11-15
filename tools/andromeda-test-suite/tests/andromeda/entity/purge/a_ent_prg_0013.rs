//! A dataset scoped purge reaches only the sources registered for its dataset or for none.

use andromeda_test_suite::{
    broker,
    dataset_sources::{DatasetBinding, DatasetBoundSources},
    namespace::Namespace,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::{DistributedOperation, EntityInfo, RegistrationInfo};

/// Pins: over inclusive registrations of `Vehicle` Entities listing the `datasetId`s [A] (source a) and [B] (source
/// b), and one listing none (source n), Purge Entities by type with drop speed and `datasetId` A answers 204 and is
/// forwarded to a and n alone
/// Clause: 5.12, 5.6.21.4, 5.6.5.4
/// Recorded: Andromeda extension, not v1.9.1: clause 5.6.21.3 gives Purge Entities no `datasetId`; the
/// `purge_dataset_scope` extension adds the one of Delete Attribute, and clause 5.12 matches a request naming a
/// `datasetId` only with the registrations listing it or listing none
#[gtest]
#[tokio::test]
async fn a_ent_prg_0013_a_dataset_scoped_purge_reaches_only_the_sources_registered_for_its_dataset_or_for_none() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let dataset_a = namespace.dataset_id("Dataset", "a")?;
    let sources = DatasetBoundSources::register(
        &broker,
        &namespace,
        &RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?,
        &[DistributedOperation::PurgeEntity],
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
    let purged_at =
        |prefix: &str| SourceRequest::new(Method::DELETE, &format!("/{prefix}/ngsi-ld/v1/entities")).query_containing("type", format!("Vehicle_{namespace}"));
    sources.answer(purged_at, |_| source_answer::status(StatusCode::NO_CONTENT)).await?;
    // Purge Entities defines no datasetId, so the parameter is written as the test needs it.
    let request = request_to(
        &broker,
        Method::DELETE,
        &format!("entities?type={vehicle_type}&drop=speed&datasetId=urn%3Angsi-ld%3ADataset%3A{namespace}-a"),
    )?;
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NO_CONTENT));
    let reached = sources.reached(|prefix| purged_at(prefix).query("datasetId", &dataset_a)).await?;
    expect_that!(reached, eq(&vec!["a", "n"]));
    Ok(())
}
