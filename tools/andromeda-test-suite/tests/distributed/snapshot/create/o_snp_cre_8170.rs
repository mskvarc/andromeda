//! A temporal capture whose peer answers more than the entity limit records too many results.

use andromeda_test_suite::{
    bounded_federation::{hold_vehicle_histories, register_temporal_peer},
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    snapshot_capture::{SnapshotQuery, await_query_problem},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::ErrorType,
    query::temporal_query::TemporalQuery,
    registration::RegistrationMode,
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId},
};

/// Pins: A Snapshot on broker-f, whose limit is 5 distinct entities, with one temporal query over a type broker-f
/// holds no history of and peer broker B, under an inclusive `queryTemporal` registration, holds the histories of 6
/// Entities of, ends failure, and its temporal query's execution details are failure with a 403 `TooManyResults`
/// problem whose detail names `federation.query.max_entities`
/// Clause: 4.12, 5.7.4.4, 5.16.1.4, 5.2.42
/// Recorded: clause 4.12 leaves the threshold of `TooManyResults` to the implementation; broker-f gathers at most 5
/// distinct entities and 15 source rows; clause 5.16.1.4 executes a Snapshot temporal query following clause 5.7.4.4,
/// so the capture records the problem Query Temporal Evolution of Entities over the same source answers with
#[gtest]
#[tokio::test]
async fn o_snp_cre_8170_a_temporal_capture_whose_peer_answers_more_than_the_entity_limit_records_too_many_results() -> Result<()> {
    let namespace = Namespace::fresh();
    let bounded = broker::client(BrokerRole::F)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    hold_vehicle_histories(&broker_b, &namespace, &[1, 2, 3, 4, 5, 6]).await?;
    let registration_id = register_temporal_peer(&bounded, &namespace, 1, PeerRole::B, RegistrationMode::Inclusive).await?;
    let _registration_cleanup = Cleanup::of(&bounded).registration(&registration_id);
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .temporal_query(
            QueryDocument::temporal_builder(TemporalQuery::after("2025-01-01T00:00:00Z".parse()?))
                .entities([vehicle_type.as_str()])
                .build()?,
        )
        .build()?;
    bounded.snapshots().create(&snapshot).send().await?;
    let problem = await_query_problem(&bounded, &snapshot_id, SettledStatus::Failure, SnapshotQuery::Temporal(0)).await?;
    expect_that!(problem.error_type, some(eq(&ErrorType::TooManyResults)));
    expect_that!(problem.status, some(eq(403)));
    expect_that!(problem.detail, some(contains_substring("federation.query.max_entities")));
    bounded.snapshots().delete(&snapshot_id).send().await?;
    bounded.registrations().delete(&registration_id).send().await?;
    Ok(())
}
