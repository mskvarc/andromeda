//! A temporal capture no peer answers an entity for captures its local entities past the entity limit.

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
/// the histories of 6 Entities of and peer broker B, under an inclusive `queryTemporal` registration, holds no
/// history of, ends success, and Query Temporal Evolution of Entities under the Snapshot answers all 6
/// Clause: 4.12, 5.7.4.4, 5.16.1.4
/// Recorded: broker-f bounds only what one query gathers from its context sources, at most 5 distinct entities and 15
/// source rows; a query no source answers an entity for is the broker's own and is paginated, never refused, as Query
/// Temporal Evolution of Entities answers it
#[gtest]
#[tokio::test]
async fn o_snp_cre_8174_a_temporal_capture_no_peer_answers_an_entity_for_captures_its_local_entities_past_the_entity_limit() -> Result<()> {
    let namespace = Namespace::fresh();
    let bounded = broker::client(BrokerRole::F)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    hold_vehicle_histories(&bounded, &namespace, &[1, 2, 3, 4, 5, 6]).await?;
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
    expect_that!(captured.items(), len(eq(6)));
    bounded.snapshots().delete(&snapshot_id).send().await?;
    bounded.registrations().delete(&registration_id).send().await?;
    Ok(())
}
