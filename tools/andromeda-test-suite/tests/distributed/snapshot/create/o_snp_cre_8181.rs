//! A temporal capture a source failed for keeps the local history and names the source.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Upserted,
    snapshot_capture::{SnapshotQuery, await_query_result},
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query::{self, TemporalQuery},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::{ExecutionStatus, QueryDocument, SettledStatus, Snapshot, SnapshotId},
};

/// Pins: A Snapshot with a temporal query after 2025-01-01 over a type the broker holds the history of Entity 1 of,
/// one `speed` instance observed at 2026-01-01, and an inclusive `queryTemporal` registration whose source answers 500
/// ends success, its temporal query's execution details are success with a problem of type
/// `https://uri.etsi.org/ngsi-ld/errors/BadGateway` and no `status` whose detail names the registration and the
/// warning code 299, and Query Temporal Evolution of Entities under the Snapshot answers the history of Entity 1
/// Clause: 5.16.1.4, 5.7.4.4, 5.2.42, 6.3.17, 5.5.3
/// Recorded: clause 5.16.1.4 executes the temporal query as Query Temporal Evolution of Entities does, which answers
/// the local history without the failed source and, in Andromeda, reports it in an `NGSILD-Warning` as Query Entities
/// does (clause 6.3.17 defines the warnings for `/entities`); Andromeda records the warnings in `problemDetails` as a
/// problem of the `BadGateway` type it answers the failure of a single source with, leaving out `status` since no
/// error response was given (clause 5.5.3)
#[gtest]
#[tokio::test]
async fn o_snp_cre_8181_a_temporal_capture_a_source_failed_for_keeps_the_local_history_and_names_the_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let history = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(1).observed_at("2026-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&history).send().await?.created()?;
    source
        .answer(
            Method::GET,
            "/a/ngsi-ld/v1/temporal/entities",
            source_answer::status(StatusCode::INTERNAL_SERVER_ERROR),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let _cleanup = Cleanup::of(&broker).entity(&vehicle_id).registration(&registration_id);
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .temporal_query(
            QueryDocument::temporal_builder(TemporalQuery::after("2025-01-01T00:00:00Z".parse()?))
                .entities([vehicle_type.as_str()])
                .build()?,
        )
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let detail = await_query_result(&broker, &snapshot_id, SettledStatus::Success, SnapshotQuery::Temporal(0)).await?;
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
    let histories = reader
        .on_snapshot(&snapshot_id)
        .temporal()
        .query(temporal_query::after("2025-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(histories.items(), elements_are![has_id(&vehicle_id)]);
    broker.snapshots().delete(&snapshot_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
