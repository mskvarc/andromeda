//! A temporal capture whose peers send more than the source row ceiling records too many results.

use andromeda_test_suite::{
    bounded_federation::{hold_vehicle_histories, register_temporal_peer},
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    snapshot_capture::{QueryList, await_query_problem},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::ErrorType,
    query::temporal_query::TemporalQuery,
    registration::RegistrationMode,
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId},
};

/// Pins: A Snapshot on broker-f, whose source row ceiling is 15, with one temporal query over a type broker-f holds no
/// history of and peer brokers B, C, D and E, under inclusive `queryTemporal` registrations, each hold the histories of
/// the same 5 Entities of, 20 rows in all, ends failure, and its temporal query's execution details are failure with a
/// 403 `TooManyResults` problem whose detail names `federation.query.max_source_rows`
/// Clause: 4.12, 5.7.4.4, 5.16.1.4, 5.2.42
/// Recorded: clause 4.12 leaves the threshold of `TooManyResults` to the implementation; broker-f gathers at most 5
/// distinct entities and 15 source rows; clause 5.16.1.4 executes a Snapshot temporal query following clause 5.7.4.4,
/// so the capture records the problem Query Temporal Evolution of Entities over the same sources answers with
#[gtest]
#[tokio::test]
async fn o_snp_cre_8171_a_temporal_capture_whose_peers_send_more_than_the_source_row_ceiling_records_too_many_results() -> Result<()> {
    let namespace = Namespace::fresh();
    let bounded = broker::client(BrokerRole::F)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let broker_c = broker::client(BrokerRole::C)?;
    let broker_d = broker::client(BrokerRole::D)?;
    let broker_e = broker::client(BrokerRole::E)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    for holder in [&broker_b, &broker_c, &broker_d, &broker_e] {
        hold_vehicle_histories(holder, &namespace, &[1, 2, 3, 4, 5]).await?;
    }
    let registration_1 = register_temporal_peer(&bounded, &namespace, 1, PeerRole::B, RegistrationMode::Inclusive).await?;
    let registration_2 = register_temporal_peer(&bounded, &namespace, 2, PeerRole::C, RegistrationMode::Inclusive).await?;
    let registration_3 = register_temporal_peer(&bounded, &namespace, 3, PeerRole::D, RegistrationMode::Inclusive).await?;
    let registration_4 = register_temporal_peer(&bounded, &namespace, 4, PeerRole::E, RegistrationMode::Inclusive).await?;
    let _registration_cleanup = Cleanup::of(&bounded)
        .registration(&registration_1)
        .registration(&registration_2)
        .registration(&registration_3)
        .registration(&registration_4);
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .temporal_query(
            QueryDocument::temporal_builder(TemporalQuery::after("2025-01-01T00:00:00Z".parse()?))
                .entities([vehicle_type.as_str()])
                .build()?,
        )
        .build()?;
    bounded.snapshots().create(&snapshot).send().await?;
    let problem = await_query_problem(&bounded, &snapshot_id, SettledStatus::Failure, QueryList::Temporal).await?;
    expect_that!(problem.error_type, some(eq(&ErrorType::TooManyResults)));
    expect_that!(problem.status, some(eq(403)));
    expect_that!(problem.detail, some(contains_substring("federation.query.max_source_rows")));
    bounded.snapshots().delete(&snapshot_id).send().await?;
    for registration_id in [&registration_1, &registration_2, &registration_3, &registration_4] {
        bounded.registrations().delete(registration_id).send().await?;
    }
    Ok(())
}
