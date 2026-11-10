//! A temporal capture a source answers partially at no range records a bad gateway.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    snapshot_capture::{SnapshotQuery, await_query_problem},
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{Lossless, Property, evolution::TemporalEntity},
    query::temporal_query::TemporalQuery,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId},
};
use wiremock::ResponseTemplate;

/// Pins: A Snapshot with a temporal query over a type the broker holds no Entity of and a `queryTemporal`
/// registration whose source answers every request 206 with the history of one Entity and no `Content-Range`, ends
/// failure, and its temporal query's execution details are failure with a 502 problem of type
/// `https://uri.etsi.org/ngsi-ld/errors/BadGateway`
/// Clause: 5.16.1.4, 5.7.4.4, 6.3.10, 5.2.42, 6.3.17
/// Recorded: clause 6.3.10 cuts a temporal answer short with a 206 whose `Content-Range` names where the next request
/// resumes, and clause 5.16.1.4 retrieves every page; a 206 with no `Content-Range` names no such instant, so the rest
/// of the source's history cannot be retrieved; clause 6.3.17 binds 502 only to the single source of an exclusive or
/// redirect registration and names no type for it, and Andromeda records a source answer it cannot read to the end as
/// that 502, typed `BadGateway` under the ETSI errors namespace
#[gtest]
#[tokio::test]
async fn o_snp_cre_8175_a_temporal_capture_a_source_answers_partially_at_no_range_records_a_bad_gateway() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let history = TemporalEntity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(1).observed_at("2026-01-01T00:00:00Z".parse()?))
        .build()?;
    source
        .answer(
            Method::GET,
            "/a/ngsi-ld/v1/temporal/entities",
            ResponseTemplate::new(StatusCode::PARTIAL_CONTENT.as_u16()).set_body_raw(format!("[{}]", history.render(Lossless::Normalized)), "application/json"),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .temporal_query(
            QueryDocument::temporal_builder(TemporalQuery::after("2025-01-01T00:00:00Z".parse()?))
                .entities([vehicle_type.as_str()])
                .build()?,
        )
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let problem = await_query_problem(&broker, &snapshot_id, SettledStatus::Failure, SnapshotQuery::Temporal(0)).await?;
    expect_that!(
        problem.error_type.as_ref().map(ToString::to_string),
        some(eq("https://uri.etsi.org/ngsi-ld/errors/BadGateway"))
    );
    expect_that!(problem.status, some(eq(502)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
