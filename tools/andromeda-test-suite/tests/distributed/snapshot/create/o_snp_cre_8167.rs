//! A capture whose peers answer more distinct entities than the limit records too many results.

use andromeda_test_suite::{
    bounded_federation::{hold_vehicles, register_peer},
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    snapshot_capture::{SnapshotQuery, await_query_problem},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::ErrorType,
    registration::RegistrationMode,
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId},
};

/// Pins: A Snapshot on broker-f, whose limit is 5 distinct entities, over a type broker-f holds no Entity of and peer
/// brokers B and D, under inclusive registrations, hold Entities 1 to 5 and 1 to 6 of, 11 rows naming 6 distinct
/// Entities, ends failure, and its query's execution details are failure with a 403 `TooManyResults` problem whose
/// detail names `federation.query.max_entities`
/// Clause: 4.12, 5.7.2.4, 5.16.1.4, 5.2.42, 5.5.2
/// Recorded: clause 4.12 leaves the threshold of `TooManyResults` to the implementation; broker-f gathers at most 5
/// distinct entities and 15 source rows; clause 5.16.1.4 executes a Snapshot query following clause 5.7.2.4, so the
/// capture records the problem Query Entities over the same sources answers with
#[gtest]
#[tokio::test]
async fn o_snp_cre_8167_a_capture_whose_peers_answer_more_distinct_entities_than_the_limit_records_too_many_results() -> Result<()> {
    let namespace = Namespace::fresh();
    let bounded = broker::client(BrokerRole::F)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let broker_d = broker::client(BrokerRole::D)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    hold_vehicles(&broker_b, &namespace, &[1, 2, 3, 4, 5]).await?;
    hold_vehicles(&broker_d, &namespace, &[1, 2, 3, 4, 5, 6]).await?;
    let registration_1 = register_peer(&bounded, &namespace, 1, PeerRole::B, RegistrationMode::Inclusive).await?;
    let registration_2 = register_peer(&bounded, &namespace, 2, PeerRole::D, RegistrationMode::Inclusive).await?;
    let _registration_cleanup = Cleanup::of(&bounded).registration(&registration_1).registration(&registration_2);
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    bounded.snapshots().create(&snapshot).send().await?;
    let problem = await_query_problem(&bounded, &snapshot_id, SettledStatus::Failure, SnapshotQuery::Entity(0)).await?;
    expect_that!(problem.error_type, some(eq(&ErrorType::TooManyResults)));
    expect_that!(problem.status, some(eq(403)));
    expect_that!(problem.detail, some(contains_substring("federation.query.max_entities")));
    bounded.snapshots().delete(&snapshot_id).send().await?;
    bounded.registrations().delete(&registration_1).send().await?;
    bounded.registrations().delete(&registration_2).send().await?;
    Ok(())
}
