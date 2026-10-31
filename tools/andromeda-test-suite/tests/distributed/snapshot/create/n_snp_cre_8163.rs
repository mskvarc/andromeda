//! A capture of thousands of local and peer entities answers concurrent reads of both.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    identity_matcher::has_id,
    namespace::Namespace,
    snapshot_capture::await_status,
    vehicle_fleet::{create_current, current_vehicles, vehicle_id},
};
use futures::future::join_all;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
};

/// The Entities each broker holds.
const VEHICLES_PER_BROKER: u32 = 2_000;

/// The Retrieve Entity requests sent at once under the Snapshot, alternately for a local and a peer Entity.
const PARALLEL_READS: u32 = 64;

/// Pins: A Snapshot on broker A over a type held by 2,000 Entities locally and by 2,000 on peer broker B, under an
/// inclusive `queryEntity` registration, ends success; Query Entities under it with count true answers
/// NGSILD-Results-Count 4000, and 64 Retrieve Entity requests under it sent at once, alternately for a local and a
/// peer Entity, each answer 200 with their Entity
/// Clause: 5.16.1.4, 5.7.2.4, 5.7.1.4, 4.3.6.2, 5.5.15, 6.3.13
#[gtest]
#[tokio::test]
async fn n_snp_cre_8163_a_capture_of_thousands_of_local_and_peer_entities_answers_concurrent_reads_of_both() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let local = current_vehicles(&namespace, "Vehicle", "local", VEHICLES_PER_BROKER)?;
    create_current(broker.entities(), &local).await?;
    let remote = current_vehicles(&namespace, "Vehicle", "remote", VEHICLES_PER_BROKER)?;
    create_current(broker_b.entities(), &remote).await?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        // Clone: the builder owns the id the test deletes the registration by.
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            // Clone: the registration owns its type, which the test keeps querying.
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let snapshot = Snapshot::builder()
        // Clone: the builder owns the id the test keeps addressing.
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    await_status(&broker, &snapshot_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    let captured = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .limit(1)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(captured.total(), some(eq(2 * u64::from(VEHICLES_PER_BROKER))));
    let read_ids = (0..PARALLEL_READS)
        .map(|index| vehicle_id(&namespace, "Vehicle", if index % 2 == 0 { "local" } else { "remote" }, index))
        .collect::<Result<Vec<_>>>()?;
    let reader = broker.with_responses().with_accept(Accept::new(NgsiLdMediaType::Json));
    let reads = join_all(
        read_ids
            .iter()
            .map(|id| async { reader.on_snapshot(&snapshot_id).entities().retrieve(id).send().await }),
    )
    .await;
    expect_that!(
        reads.iter().map(AnsweredStatus::answered_status).collect::<Vec<_>>(),
        each(some(eq(&StatusCode::OK)))
    );
    for (id, read) in read_ids.iter().zip(reads) {
        expect_that!(read?.outcome.body(), has_id(id));
    }
    broker.snapshots().delete(&snapshot_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
