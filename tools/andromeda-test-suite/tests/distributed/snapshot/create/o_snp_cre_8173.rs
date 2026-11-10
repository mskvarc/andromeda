//! A temporal capture whose local and peer entities reach the entity limit together captures them all.

use andromeda_test_suite::{
    bounded_federation::{hold_vehicle_histories, register_temporal_peer},
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    namespace::Namespace,
    snapshot_capture::await_status,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::temporal_query::{self, TemporalQuery},
    registration::RegistrationMode,
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot on broker-f, whose limit is 5 distinct entities, with one temporal query over a type broker-f holds
/// the histories of Entities 1 to 3 of and peer broker B, under an inclusive `queryTemporal` registration, holds the
/// histories of Entities 3 to 5 of, 5 merged with Entity 3 counted once, ends success, and Query Temporal Evolution
/// of Entities under the Snapshot answers all 5
/// Clause: 4.12, 5.7.4.4, 5.16.1.4
/// Recorded: clause 4.12 leaves the threshold of `TooManyResults` to the implementation; broker-f gathers at most 5
/// distinct entities and 15 source rows; clause 5.7.4.4 merges an entity a source and the broker both hold into one,
/// so the merged collection reaches the limit without passing it
#[gtest]
#[tokio::test]
async fn o_snp_cre_8173_a_temporal_capture_whose_local_and_peer_entities_reach_the_entity_limit_together_captures_them_all() -> Result<()> {
    let namespace = Namespace::fresh();
    let bounded = broker::client(BrokerRole::F)?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    hold_vehicle_histories(&bounded, &namespace, &[1, 2, 3]).await?;
    hold_vehicle_histories(&broker_b, &namespace, &[3, 4, 5]).await?;
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
    await_status(&bounded, &snapshot_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    let captured = bounded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .temporal()
        .query(temporal_query::after("2025-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    let mut captured_ids: Vec<String> = captured.items().iter().map(|entity| entity.id.to_string()).collect();
    captured_ids.sort_unstable();
    let mut expected_ids = Vec::new();
    for number in 1..=5 {
        expected_ids.push(namespace.urn("Vehicle", number)?.to_string());
    }
    expected_ids.sort_unstable();
    expect_that!(captured_ids, eq(&expected_ids));
    bounded.snapshots().delete(&snapshot_id).send().await?;
    bounded.registrations().delete(&registration_id).send().await?;
    Ok(())
}
