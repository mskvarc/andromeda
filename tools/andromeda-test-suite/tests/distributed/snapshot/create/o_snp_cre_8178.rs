//! A capture a source failed for keeps the local entities and names the source.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    snapshot_capture::{SnapshotQuery, await_query_result},
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::{ExecutionStatus, QueryDocument, SettledStatus, Snapshot, SnapshotId},
};

/// Pins: A Snapshot over a type the broker holds Entity 1 of and an inclusive `queryEntity` registration whose source
/// answers 500 ends success, its query's execution details are success with a problem of type
/// `https://uri.etsi.org/ngsi-ld/errors/BadGateway` and no `status` whose detail names the registration and the
/// warning code 299, and Query Entities under the Snapshot answers Entity 1
/// Clause: 5.16.1.4, 5.7.2.4, 5.2.42, 6.3.17, 5.5.3
/// Recorded: clause 5.16.1.4 executes the query as Query Entities does, which answers the local Entity without the
/// failed inclusive source and reports the source in an `NGSILD-Warning` (clause 6.3.17); an execution result has no
/// member for warnings, and Andromeda records them in `problemDetails` as a problem of the `BadGateway` type it answers
/// the failure of a single source with, leaving out `status` since no error response was given (clause 5.5.3)
#[gtest]
#[tokio::test]
async fn o_snp_cre_8178_a_capture_a_source_failed_for_keeps_the_local_entities_and_names_the_source() -> Result<()> {
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
        .answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::status(StatusCode::INTERNAL_SERVER_ERROR))
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
    let problem = detail.problem_details.or_fail()?;
    expect_that!(
        problem.error_type.as_ref().map(ToString::to_string),
        some(eq("https://uri.etsi.org/ngsi-ld/errors/BadGateway"))
    );
    expect_that!(problem.status, none());
    expect_that!(
        problem.detail,
        some(all!(contains_substring(registration_id.to_string()), contains_substring("299 - ")))
    );
    let reader = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let vehicles = reader.on_snapshot(&snapshot_id).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(vehicles.items(), elements_are![has_id(&vehicle_id)]);
    broker.snapshots().delete(&snapshot_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
