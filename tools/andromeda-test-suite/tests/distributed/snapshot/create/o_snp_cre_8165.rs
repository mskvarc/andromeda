//! A capture whose local and peer entities together pass the entity limit records too many results.

use andromeda_test_suite::{
    bounded_federation::{hold_vehicles, register_peer},
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    eventually::retried,
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::RegistrationMode,
    snapshot::{ExecutionStatus, QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot on broker-f, whose limit is 5 distinct entities, over a type broker-f holds Entities 1 to 3 of
/// and peer broker B, under an inclusive registration, holds Entities 4 to 6 of, 3 Entities each and 6 merged, ends
/// failure, and its query's execution details are failure with a problem whose detail names
/// `federation.query.max_entities`
/// Clause: 4.12, 5.7.2.4, 5.16.1.4, 5.2.42
/// Recorded: clause 4.12 leaves the threshold of `TooManyResults` to the implementation; broker-f gathers at most 5
/// distinct entities and 15 source rows; clause 5.16.1.4 executes a Snapshot query following clause 5.7.2.4, so the
/// capture is refused as Query Entities over the same merged collection is
#[gtest]
#[tokio::test]
async fn o_snp_cre_8165_a_capture_whose_local_and_peer_entities_together_pass_the_entity_limit_records_too_many_results() -> Result<()> {
    let namespace = Namespace::fresh();
    let bounded = broker::client(BrokerRole::F)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    hold_vehicles(&bounded, &namespace, &[1, 2, 3]).await?;
    hold_vehicles(&broker_b, &namespace, &[4, 5, 6]).await?;
    let registration_id = register_peer(&bounded, &namespace, 1, PeerRole::B, RegistrationMode::Inclusive).await?;
    let _registration_cleanup = Cleanup::of(&bounded).registration(&registration_id);
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    bounded.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let settled = bounded
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(settled.status(), eq("failure".parse::<SnapshotStatus>()?))?;
        verify_that!(settled.output().queries_details, len(eq(1)))?;
        verify_that!(
            settled.output().queries_details.first().map(|details| details.result_status),
            some(eq(ExecutionStatus::Failure))
        )?;
        verify_that!(
            settled
                .output()
                .queries_details
                .first()
                .and_then(|details| details.problem_details.as_ref())
                .and_then(|problem| problem.detail.as_deref()),
            some(contains_substring("federation.query.max_entities"))
        )?;
        Ok(())
    })
    .await?;
    bounded.snapshots().delete(&snapshot_id).send().await?;
    bounded.registrations().delete(&registration_id).send().await?;
    Ok(())
}
