//! A temporal capture of thousands of local and peer instances answers concurrent reads of both.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    snapshot_capture::await_status,
    vehicle_fleet::{OBSERVATIONS_PER_VEHICLE, observation_totals, observed_vehicles, record_observed, vehicle_id},
};
use futures::future::join_all;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        DateTime,
        NgsiLdMediaType,
        evolution::{AggregationMethod, TemporalAttributeName},
    },
    query::temporal_query::{self, TemporalQuery},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
};

/// The Temporal Evolutions each broker holds.
const VEHICLES_PER_BROKER: u32 = 500;

/// The Retrieve Temporal Evolution requests sent at once under the Snapshot, alternately for a local and a peer
/// Entity.
const PARALLEL_READS: u32 = 64;

/// Pins: A Snapshot on broker A with a temporal query over a type whose Temporal Evolution is held for 500 Entities
/// locally and for 500 on peer broker B, under an inclusive `queryTemporal` registration, 20 instances each, ends
/// success; under it the 1,000 Temporal Evolutions each count 20 instances, and 64 Retrieve Temporal Evolution
/// requests with `lastN` 20 sent at once, alternately for a local and a peer Entity, each answer 200 with their
/// Entity's 20 instances
/// Clause: 5.16.1.4, 5.7.4.4, 5.7.3.4, 4.3.6.2, 4.5.19, 5.5.15
#[gtest]
#[tokio::test]
async fn n_snp_cre_8164_a_temporal_capture_of_thousands_of_local_and_peer_instances_answers_concurrent_reads_of_both() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let since = "2024-01-01T00:00:00Z".parse::<DateTime>()?;
    let speed = "speed".parse::<TemporalAttributeName>()?;
    let local = observed_vehicles(&namespace, "Vehicle", "local", VEHICLES_PER_BROKER, 0)?;
    record_observed(broker.temporal(), &local).await?;
    let remote = observed_vehicles(&namespace, "Vehicle", "remote", VEHICLES_PER_BROKER, VEHICLES_PER_BROKER)?;
    record_observed(broker_b.temporal(), &remote).await?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        // Clone: the builder owns the id the test deletes the registration by.
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            // Clone: the registration owns its type, which the test keeps querying.
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let snapshot = Snapshot::builder()
        // Clone: the builder owns the id the test keeps addressing.
        .id(snapshot_id.clone())
        .temporal_query(
            QueryDocument::temporal_builder(TemporalQuery::after(since))
                .entities([vehicle_type.as_str()])
                .build()?,
        )
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    await_status(&broker, &snapshot_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let totals = observation_totals(json.on_snapshot(&snapshot_id).temporal(), &vehicle_type, 2 * VEHICLES_PER_BROKER).await?;
    expect_that!(totals, len(eq(usize::try_from(2 * VEHICLES_PER_BROKER)?)));
    expect_that!(
        totals,
        each(has_aggregate(
            "speed",
            AggregationMethod::TotalCount,
            all!(len(eq(1)), nth(0, period().value(i64::from(OBSERVATIONS_PER_VEHICLE))))
        ))
    );
    let read_ids = (0..PARALLEL_READS)
        .map(|index| vehicle_id(&namespace, "Vehicle", if index % 2 == 0 { "local" } else { "remote" }, index))
        .collect::<Result<Vec<_>>>()?;
    let reader = broker.with_heads().with_accept(Accept::new(NgsiLdMediaType::Json));
    let reads = join_all(read_ids.iter().map(|id| async {
        reader
            .on_snapshot(&snapshot_id)
            .temporal()
            .retrieve(id)
            .temporal_q(temporal_query::after(since).last_n(u64::from(OBSERVATIONS_PER_VEHICLE)))
            // Clone: each request owns its attribute list.
            .attrs(vec![speed.clone()])
            .send()
            .await
    }))
    .await;
    for (id, read) in read_ids.iter().zip(reads) {
        let read = read?;
        expect_that!(read.status, eq(StatusCode::OK));
        expect_that!(read.outcome.body(), has_id(id));
        expect_that!(read.outcome.body(), has_evolution("speed", len(eq(usize::try_from(OBSERVATIONS_PER_VEHICLE)?))));
    }
    broker.snapshots().delete(&snapshot_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
