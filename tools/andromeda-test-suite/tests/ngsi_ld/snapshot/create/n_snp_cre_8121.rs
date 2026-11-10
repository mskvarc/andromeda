//! Execution details carry a problem only for the query whose only source cannot be reached.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, eventually::retried, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
    snapshot::{ExecutionStatus, QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: The `snapshotQueriesDetails` of a Snapshot with a successful query, an empty query and a query whose only
/// source cannot be reached are success, empty and empty, and only the third carries `problemDetails`: a problem of
/// type `https://uri.etsi.org/ngsi-ld/errors/BadGateway` and no `status`
/// Clause: 5.16.1.4, 5.2.42, 6.3.17, 5.5.3
/// Recorded: clause 5.16.1.4 executes the query as Query Entities does, which answers without a source it cannot reach
/// and reports the source in an `NGSILD-Warning` (clause 6.3.17), so the query has not failed; an execution result has
/// no member for warnings, and Andromeda records them in `problemDetails` as a problem of the `BadGateway` type it
/// answers the failure of a single source with, leaving out `status` since no error response was given (clause 5.5.3)
#[gtest]
#[tokio::test]
async fn n_snp_cre_8121_execution_details_carry_a_problem_only_for_the_query_whose_only_source_cannot_be_reached() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let fail_type = namespace.entity_type("Fail")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let absent_type = namespace.entity_type("Absent")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(fail_type.clone()).with_id_pattern(namespace.id_pattern("Fail")?)])
            .property_names([namespace.attribute("speed")?.to_string()])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 10)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .query(QueryDocument::builder().entities([absent_type.as_str()]).build()?)
        .query(QueryDocument::builder().entities([fail_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let snapshot_2 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(snapshot_2.status(), eq("partial".parse::<SnapshotStatus>()?))?;
        verify_that!(snapshot_2.output().queries_details, len(eq(3)))?;
        verify_that!(
            snapshot_2.output().queries_details.first().map(|details| details.result_status),
            some(eq(ExecutionStatus::Success))
        )?;
        verify_that!(
            snapshot_2.output().queries_details.first().map(|details| &details.problem_details),
            some(none())
        )?;
        verify_that!(
            snapshot_2.output().queries_details.get(1).map(|details| details.result_status),
            some(eq(ExecutionStatus::Empty))
        )?;
        verify_that!(snapshot_2.output().queries_details.get(1).map(|details| &details.problem_details), some(none()))?;
        verify_that!(
            snapshot_2.output().queries_details.get(2).map(|details| details.result_status),
            some(eq(ExecutionStatus::Empty))
        )?;
        let problem = snapshot_2.output().queries_details.get(2).and_then(|details| details.problem_details.as_ref());
        verify_that!(
            problem.and_then(|problem| problem.error_type.as_ref()).map(ToString::to_string),
            some(eq("https://uri.etsi.org/ngsi-ld/errors/BadGateway"))
        )?;
        verify_that!(problem.and_then(|problem| problem.status), none())?;
        Ok(())
    })
    .await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
