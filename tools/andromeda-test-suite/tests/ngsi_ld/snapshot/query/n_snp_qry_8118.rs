//! Temporal provision under a snapshot changes the snapshot history alone.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    eventually::retried,
    evolution_matcher::{has_evolution, lacks_evolution},
    namespace::Namespace,
    outcome::Upserted,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, NgsiLdMediaType, Property, evolution::TemporalEntity},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: Create Temporal Evolution, Add Attribute Instances, Delete Attribute and Delete Temporal Evolution with
/// NGSILD-Snapshot answer 201, 204, 204 and 204: the history under the Snapshot holds both instances, then no
/// speed, then nothing, and the Entity has no history live
/// Clause: 5.5.15, 5.6.11.4, 5.6.12.4, 5.6.13.4, 5.6.16.4, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_snp_qry_8118_temporal_provision_under_a_snapshot_changes_the_snapshot_history_alone() -> Result<()> {
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
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(10).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker
        .on_snapshot(&snapshot_id)
        .temporal()
        .create_or_update(&vehicle_evolution)
        .send()
        .await?
        .created()?;
    let instances = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(20).observed_at("2025-01-01T01:00:00Z".parse()?))
        .build()?
        .attributes;
    broker
        .on_snapshot(&snapshot_id)
        .temporal()
        .add_attributes(&vehicle_id, &instances)
        .send()
        .await?;
    let retrieved = broker
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
            all!(len(eq(2)), contains(any_attribute().value(10)), contains(any_attribute().value(20)))
        )
    );
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    broker
        .on_snapshot(&snapshot_id)
        .temporal()
        .delete_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?)
        .send()
        .await?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2, lacks_evolution("speed"));
    broker.on_snapshot(&snapshot_id).temporal().delete(&vehicle_id).send().await?;
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
