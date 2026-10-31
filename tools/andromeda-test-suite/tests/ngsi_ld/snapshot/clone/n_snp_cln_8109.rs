//! A clone of a large mixed snapshot is complete and independent while its source answers concurrent reads.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    answered_status::AnsweredStatus,
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::problem,
    snapshot_capture::await_status,
    vehicle_fleet::{OBSERVATIONS_PER_VEHICLE, create_current, current_vehicles, observation_totals, observed_vehicles, record_observed, vehicle_id},
};
use futures::future::join_all;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{ErrorType, NgsiLdMediaType, evolution::AggregationMethod},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotClone, SnapshotId, SnapshotStatus},
};

/// The Entities written to the source Snapshot.
const CURRENT_VEHICLES: u32 = 4_000;

/// The Temporal Evolutions written to the source Snapshot: with [`CURRENT_VEHICLES`], 5,000 Entities and 20,000
/// written instances. A broker that also records the Core creates as history holds 24,000 instances, but whether it
/// does is its own choice (clause 4.3.1), so the clone is checked for the 20,000 every broker holds.
const OBSERVED_VEHICLES: u32 = 1_000;

/// The Retrieve Entity requests sent at once to the source while its clone is prepared.
const PARALLEL_READS: u32 = 64;

/// Pins: A Snapshot holding 4,000 Entities and the Temporal Evolution of 1,000 Entities of 20 instances each, all
/// written to it, is cloned; 64 Retrieve Entity requests sent at once to the source right after the clone answers
/// 201 each answer 200 with their Entity; the clone ends success, source and clone each answer 4,000 Entities and
/// 1,000 Temporal Evolutions whose `totalCount` is 20 apiece; an Entity deleted from the source afterwards remains in
/// the clone
/// Clause: 5.16.2.4, 5.5.15, 5.7.4.4, 4.5.19, 6.3.13
#[gtest]
#[tokio::test]
async fn n_snp_cln_8109_a_clone_of_a_large_mixed_snapshot_is_complete_and_independent_while_its_source_answers_concurrent_reads() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let car_type = namespace.entity_type("Car")?;
    let truck_type = namespace.entity_type("Truck")?;
    let source_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let clone_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    let source = Snapshot::builder()
        // Clone: the builder owns the id the test keeps addressing.
        .id(source_id.clone())
        .query(QueryDocument::builder().entities([car_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&source).send().await?;
    await_status(&broker, &source_id, SnapshotStatus::Settled(SettledStatus::Empty)).await?;
    let cars = current_vehicles(&namespace, "Car", "fleet", CURRENT_VEHICLES)?;
    create_current(broker.on_snapshot(&source_id).entities(), &cars).await?;
    let trucks = observed_vehicles(&namespace, "Truck", "fleet", OBSERVED_VEHICLES, 0)?;
    record_observed(broker.on_snapshot(&source_id).temporal(), &trucks).await?;
    broker
        .snapshots()
        // Clone: the builder owns the id the test keeps addressing.
        .clone_snapshot(&source_id, &SnapshotClone::builder().id(clone_id.clone()).build()?)
        .send()
        .await?;
    let reader = broker.with_responses().with_accept(Accept::new(NgsiLdMediaType::Json));
    let read_ids = (0..PARALLEL_READS)
        .map(|index| vehicle_id(&namespace, "Car", "fleet", index))
        .collect::<Result<Vec<_>>>()?;
    let reads = join_all(
        read_ids
            .iter()
            .map(|id| async { reader.on_snapshot(&source_id).entities().retrieve(id).send().await }),
    )
    .await;
    expect_that!(
        reads.iter().map(AnsweredStatus::answered_status).collect::<Vec<_>>(),
        each(some(eq(&StatusCode::OK)))
    );
    for (id, read) in read_ids.iter().zip(reads) {
        expect_that!(read?.outcome.body(), has_id(id));
    }
    await_status(&broker, &clone_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for branch in [&source_id, &clone_id] {
        let cars_held = json
            .on_snapshot(branch)
            .entities()
            .query()
            .entity_type(&car_type)
            .limit(1)
            .count(ResultsCount::Include)
            .send()
            .await?;
        expect_that!(cars_held.total(), some(eq(u64::from(CURRENT_VEHICLES))));
        let totals = observation_totals(json.on_snapshot(branch).temporal(), &truck_type, OBSERVED_VEHICLES).await?;
        expect_that!(totals, len(eq(usize::try_from(OBSERVED_VEHICLES)?)));
        expect_that!(
            totals,
            each(has_aggregate(
                "speed",
                AggregationMethod::TotalCount,
                all!(len(eq(1)), nth(0, period().value(i64::from(OBSERVATIONS_PER_VEHICLE))))
            ))
        );
    }
    let first_car = vehicle_id(&namespace, "Car", "fleet", 0)?;
    broker.on_snapshot(&source_id).entities().delete(&first_car).send().await?.done()?;
    let deleted = reader.on_snapshot(&source_id).entities().retrieve(&first_car).send().await;
    expect_that!(&deleted, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let kept = json.on_snapshot(&clone_id).entities().retrieve(&first_car).send().await?;
    expect_that!(kept.body(), has_id(&first_car));
    broker.snapshots().delete(&clone_id).send().await?;
    broker.snapshots().delete(&source_id).send().await?;
    Ok(())
}
