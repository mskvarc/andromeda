//! A capture whose every matching source failed is empty and names the sources.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    snapshot_capture::{SnapshotQuery, await_query_result},
};
use googletest::prelude::*;
use ngsi_ld::{
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::{ExecutionStatus, QueryDocument, SettledStatus, Snapshot, SnapshotId},
};

/// Pins: A Snapshot over a type the broker holds no Entity of and an inclusive `queryEntity` registration naming an
/// origin nothing listens at ends empty, and its query's execution details are empty with a problem of type
/// `https://uri.etsi.org/ngsi-ld/errors/BadGateway` and no `status` whose detail names the registration and the
/// warning code 199
/// Clause: 5.16.1.4, 5.7.2.4, 5.2.42, 6.3.17, 5.5.3
/// Recorded: clause 5.16.1.4 executes the query as Query Entities does, which answers without a failed inclusive
/// source and reports it in an `NGSILD-Warning` (clause 6.3.17), so the query does not fail; an execution result has no
/// member for warnings, and Andromeda records them in `problemDetails` as a problem of the `BadGateway` type it answers
/// the failure of a single source with, leaving out `status` since no error response was given (clause 5.5.3 requires
/// only `type`, `title` and `detail`)
#[gtest]
#[tokio::test]
async fn o_snp_cre_8169_a_capture_whose_every_matching_source_failed_is_empty_and_names_the_sources() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(vehicle_type.clone())]).build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // A forward to a registration left behind fails at its endpoint, and the cooldown that opens there holds back a
    // later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let detail = await_query_result(&broker, &snapshot_id, SettledStatus::Empty, SnapshotQuery::Entity(0)).await?;
    expect_that!(detail.result_status, eq(ExecutionStatus::Empty));
    let problem = detail.problem_details.or_fail()?;
    expect_that!(
        problem.error_type.as_ref().map(ToString::to_string),
        some(eq("https://uri.etsi.org/ngsi-ld/errors/BadGateway"))
    );
    expect_that!(problem.status, none());
    expect_that!(
        problem.detail,
        some(all!(contains_substring(registration_id.to_string()), contains_substring("199 - ")))
    );
    broker.snapshots().delete(&snapshot_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
