//! An operation under a snapshot preparing past the wait is refused with retry after.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, SuiteConfig},
    context_source::ContextSource,
    eventually::retried,
    namespace::Namespace,
    problem::{problem, service_unavailable},
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};
use std::time::Duration;

/// Pins: On a broker that holds an operation scoped to a preparing Snapshot for two seconds, Query Entities under a
/// Snapshot whose capture waits four seconds for its context source answers 503 `ServiceUnavailable`, titled "Service
/// unavailable", with Retry-After 1, and the Snapshot then ends empty once the source has answered
/// Clause: 5.5.15, 5.16.1.4, 6.3.2
/// Recorded: Table 6.3.2-1 binds `InternalError` to 500, a failure a client does not retry, so a retryable 503 carries
/// a type of its own
/// Tenant: private (the quota of the broker counts every Snapshot of the Tenant)
#[gtest]
#[tokio::test]
async fn a_snp_qry_8140_an_operation_under_a_snapshot_preparing_past_the_wait_is_refused_with_retry_after() -> Result<()> {
    let namespace = Namespace::fresh();
    let source = ContextSource::start().await?;
    let config = SuiteConfig::get()?;
    let broker_snapshot = broker::client(BrokerRole::Durable)?;
    let broker_snapshot_tenant = broker_snapshot.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let provisioning_sentinel_type = namespace.entity_type("ProvisioningSentinel")?;
    let registration_id = namespace.registration_id(1)?;
    let slow_type = namespace.entity_type("Slow")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([provisioning_sentinel_type.as_str()]).build()?])
        .build()?;
    broker_snapshot_tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&broker_snapshot_tenant).registration(&registration_provision_id);
    broker_snapshot_tenant.registrations().delete(&registration_provision_id).send().await?;
    let source_entities = [];
    source
        .answer_when(
            SourceRequest::any_matching("/slow/.*"),
            source_answer::entities(&source_entities).set_delay(Duration::from_millis(4000)),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("slow")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(slow_type.clone()).with_id_pattern(namespace.id_pattern("Slow")?)])
            .property_names([namespace.attribute("speed")?.to_string()])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker_snapshot_tenant.registrations().register(&registration).send().await?;
    let _registration_cleanup = Cleanup::of(&broker_snapshot_tenant).registration(&registration_id);
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([slow_type.as_str()]).build()?)
        .build()?;
    broker_snapshot_tenant.snapshots().create(&snapshot).send().await?;
    let answer = broker_snapshot_tenant
        .with_responses()
        .with_accept(Accept::any())
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&slow_type)
        .send()
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::SERVICE_UNAVAILABLE, service_unavailable()).with_header(HeaderName::from_static("retry-after"), "1")
    );
    expect_that!(
        answer.answered_problem().and_then(|details| details.title.as_deref()),
        some(eq("Service unavailable"))
    );
    retried(async || {
        let snapshot_2 = broker_snapshot_tenant
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(snapshot_2.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    broker_snapshot_tenant.registrations().delete(&registration_id).send().await?;
    broker_snapshot_tenant.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
