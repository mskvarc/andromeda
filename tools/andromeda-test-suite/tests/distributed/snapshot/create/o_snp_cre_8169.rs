//! A capture whose every matching source failed records a bad gateway.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    snapshot_capture::{SnapshotQuery, await_query_problem},
};
use googletest::prelude::*;
use ngsi_ld::{
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId},
};

/// Pins: A Snapshot over a type the broker holds no Entity of and an inclusive `queryEntity` registration names an
/// origin nothing listens at for, ends failure, and its query's execution details are failure with a 502 problem of
/// type `https://uri.etsi.org/ngsi-ld/errors/BadGateway`
/// Clause: 5.16.1.4, 5.7.2.4, 5.2.42, 6.3.17
/// Recorded: clause 6.3.17 answers a failed registered source with 502, 504 or 508 and names no type for them;
/// Andromeda types them under the ETSI errors namespace, `BadGateway` for 502; a capture with no local Entity and no
/// source that answered has no result to record, so it reports the sources' failure
#[gtest]
#[tokio::test]
async fn o_snp_cre_8169_a_capture_whose_every_matching_source_failed_records_a_bad_gateway() -> Result<()> {
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
    let problem = await_query_problem(&broker, &snapshot_id, SettledStatus::Failure, SnapshotQuery::Entity(0)).await?;
    expect_that!(
        problem.error_type.as_ref().map(ToString::to_string),
        some(eq("https://uri.etsi.org/ngsi-ld/errors/BadGateway"))
    );
    expect_that!(problem.status, some(eq(502)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
