//! A capture a source answers not found for records no problem.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    outcome::Done,
    snapshot_capture::{SnapshotQuery, await_query_result},
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::{ExecutionStatus, QueryDocument, SettledStatus, Snapshot, SnapshotId},
};

/// Pins: A Snapshot over a type the broker holds Entity 1 of and an inclusive `queryEntity` registration whose source
/// answers 404 ends success, the capture contacts the source, and its query's execution details are success with no
/// `problemDetails`
/// Clause: 5.16.1.4, 5.7.2.4, 5.2.42, 6.3.17
/// Recorded: clause 6.3.17 does not consider a source answering 404 abnormal, so Query Entities sends no warning for it,
/// and Andromeda records a problem beside a query's result only for the sources Query Entities warns about
#[gtest]
#[tokio::test]
async fn o_snp_cre_8179_a_capture_a_source_answers_not_found_for_records_no_problem() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::status(StatusCode::NOT_FOUND))
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let _cleanup = Cleanup::of(&broker).entity(&vehicle_id).registration(&registration_id);
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let detail = await_query_result(&broker, &snapshot_id, SettledStatus::Success, SnapshotQuery::Entity(0)).await?;
    expect_that!(detail.result_status, eq(ExecutionStatus::Success));
    expect_that!(detail.problem_details, none());
    let contacted = source.received_matching(&SourceRequest::any_matching("/a/.*")).await?;
    expect_that!(contacted, not(is_empty()));
    broker.snapshots().delete(&snapshot_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
