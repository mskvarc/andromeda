//! A core write under a snapshot is recorded in the snapshot history.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    eventually::retried,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::temporal_query::TemporalProperty,
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: Create Entity with NGSILD-Snapshot on a broker recording Core writes adds the Entity to the Temporal
/// Evolution under the Snapshot, while the Entity has no Temporal Evolution live
/// Clause: 5.5.15, 5.6.1.4, 5.7.3.4
#[gtest]
#[tokio::test]
async fn a_snp_qry_8141_a_core_write_under_a_snapshot_is_recorded_in_the_snapshot_history() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let snapshot_2 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(snapshot_2.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 12)
        .build()?;
    broker.on_snapshot(&snapshot_id).entities().create(&vehicle).send().await?.done()?;
    retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .on_snapshot(&snapshot_id)
            .temporal()
            .retrieve(&vehicle_id)
            .time_property(TemporalProperty::ModifiedAt)
            .send()
            .await?;
        let evolution = retrieved.body();
        verify_that!(evolution, has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(12)))))?;
        Ok(())
    })
    .await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
