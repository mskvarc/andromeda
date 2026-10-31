//! An operation under a preparing snapshot waits for its capture.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, snapshot_capture::await_status};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
};
use std::time::{Duration, Instant};
use wiremock::{Mock, ResponseTemplate, matchers::any};

/// The delay the context source takes to answer the capture.
const SOURCE_DELAY: Duration = Duration::from_secs(3);

/// The least time the held operation may take: half the source's delay.
const HELD_AT_LEAST: Duration = Duration::from_millis(1500);

/// Pins: Query Entities with NGSILD-Snapshot naming a Snapshot whose capture waits three seconds for its context
/// source answers only once the capture has ended, with the captured (empty) result, and the source is contacted
/// once, by the capture alone
/// Clause: 5.5.15, 5.16.1.4
/// Recorded: clause 5.5.15 applies an operation scoped to a Snapshot to the Snapshot's information, which a
/// preparing Snapshot does not hold yet, and names no behaviour for that case; Andromeda holds the operation until
/// the preparation ends
/// Tenant: private (it counts every request its context source receives, and the discovery and unscoped requests of
/// other tests reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn o_snp_qry_8123_an_operation_under_a_preparing_snapshot_waits_for_its_capture() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let broker = broker::private_tenant(&front, &namespace).await?;
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
    let started = Instant::now();
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&slow_type)
        .send()
        .await?;
    expect_that!(started.elapsed(), ge(HELD_AT_LEAST));
    expect_that!(page.items(), is_empty());
    await_status(&broker, &snapshot_id, SnapshotStatus::Settled(SettledStatus::Empty)).await?;
    expect_that!(source.received().await?, len(eq(1)));
    broker.registrations().delete(&registration_id).send().await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
