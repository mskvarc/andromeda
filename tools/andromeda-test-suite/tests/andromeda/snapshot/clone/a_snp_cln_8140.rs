//! A core write to a clone is recorded in the clone history.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    eventually::retried,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::problem,
    snapshot_capture::await_status,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, Input, NgsiLdMediaType, Property},
    query::temporal_query::TemporalProperty,
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotClone, SnapshotId, SnapshotStatus},
};

/// Pins: Create Entity with NGSILD-Snapshot naming a clone, on a broker recording Core writes, adds the Entity to
/// the clone's Temporal Evolution, and the Snapshot it was cloned from holds no such Entity
/// Clause: 5.16.2.4, 5.5.15, 5.7.3.4
#[gtest]
#[tokio::test]
async fn a_snp_cln_8140_a_core_write_to_a_clone_is_recorded_in_the_clone_history() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let car_type = namespace.entity_type("Car")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let clone_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    let cars = (1..=2)
        .map(|number| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Car", format!("captured-{number}"))?)
                .entity_type(car_type.clone())
                .property("speed", number)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&cars).send().await?.done()?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([car_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    await_status(&broker, &snapshot_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    broker
        .snapshots()
        .clone_snapshot(&snapshot_id, &SnapshotClone::builder().id(clone_id.clone()).build()?)
        .send()
        .await?;
    await_status(&broker, &clone_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    let written_id = namespace.urn("Car", "clone-write")?;
    let written = Entity::builder()
        .id(written_id.clone())
        .entity_type(car_type.clone())
        .property("speed", Property::builder(20))
        .build()?;
    broker.on_snapshot(&clone_id).entities().create(&written).send().await?.done()?;
    retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .on_snapshot(&clone_id)
            .temporal()
            .retrieve(&written_id)
            .time_property(TemporalProperty::ModifiedAt)
            .send()
            .await?;
        verify_that!(retrieved.body(), has_id(&written_id))?;
        verify_that!(retrieved.body(), has_evolution("speed", all!(len(eq(1)), nth(0, property().value(20)))))
    })
    .await?;
    let original = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .temporal()
        .retrieve(&written_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await;
    expect_that!(&original, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    broker.snapshots().delete(&clone_id).send().await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
