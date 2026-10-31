//! A preparing snapshot renders its status and timestamps.

use andromeda_test_suite::{broker, context_source::ContextSource, eventually::retried, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
};
use std::time::Duration;
use wiremock::{Mock, ResponseTemplate, matchers::any};

/// The delay the context source takes to answer the capture.
const SOURCE_DELAY: Duration = Duration::from_secs(3);

/// Pins: A Snapshot whose capture waits for its context source reads preparing with `createdAt`, `modifiedAt`,
/// `expiresAt` and `lastUsedAt` as `DateTimes`, and ends empty with one `snapshotQueriesDetails` element and no
/// member outside the Snapshot data type
/// Clause: 5.16.1.4, 5.16.3.4, Table 5.2.41-2
#[gtest]
#[tokio::test]
async fn n_snp_sta_8103_a_preparing_snapshot_renders_its_status_and_timestamps() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let slow_type = namespace.entity_type("Slow")?;
    let speed = namespace.attribute("speed")?;
    let registration_id = namespace.registration_id(1)?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    source
        .mount(
            Mock::given(any()).respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Type", "application/json")
                    .set_body_string("[]")
                    .set_delay(SOURCE_DELAY),
            ),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("slow")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(slow_type.clone()).with_id_pattern(namespace.id_pattern("Slow")?)])
            .property_names([speed.to_string()])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([slow_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let preparing = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    expect_that!(preparing.status(), eq(SnapshotStatus::Preparing));
    expect_that!(preparing.output().created_at, some(anything()));
    expect_that!(preparing.output().modified_at, some(anything()));
    expect_that!(preparing.output().last_used_at, some(anything()));
    // The reader refuses a member outside the Snapshot data type, so a settled status read proves `preparation` and
    // `queriesDetails` absent.
    retried(async || {
        let ended = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(ended.status(), eq(SnapshotStatus::Settled(SettledStatus::Empty)))?;
        verify_that!(ended.output().queries_details, len(eq(1)))?;
        Ok(())
    })
    .await?;
    broker.registrations().delete(&registration_id).send().await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
