//! A temporal write under a snapshot is in its history at once on a deferring broker.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    config::BrokerRole,
    eventually::retried,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: Create Temporal Evolution with NGSILD-Snapshot on a broker that queues its live history answers 201, and a
/// broker reading history from storage over the same database answers the observation under the Snapshot at once
/// Clause: 5.5.15, 5.6.11.4, 5.7.3.4
#[gtest]
#[tokio::test]
async fn a_snp_qry_8142_a_temporal_write_under_a_snapshot_is_in_its_history_at_once_on_a_deferring_broker() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_deferred = broker::client(BrokerRole::Deferred)?;
    let broker_recorded = broker::client(BrokerRole::Recorded)?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker_deferred.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let captured = broker_deferred
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(captured.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    let vehicle = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(42).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker_deferred
        .on_snapshot(&snapshot_id)
        .temporal()
        .create_or_update(&vehicle)
        .send()
        .await?
        .created()?;
    let retrieved = broker_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(len(eq(1)), contains(property().value(42).observed_at("2025-01-01T00:00:00Z".parse()?)))
        )
    );
    broker_deferred.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
