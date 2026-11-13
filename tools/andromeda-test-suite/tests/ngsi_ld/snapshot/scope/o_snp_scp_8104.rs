//! An entity map operation naming a snapshot is refused and changes nothing.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    namespace::Namespace,
    outcome::Done,
    received_status::ExpectedStatus,
    snapshot_refusal::refused_on_snapshot,
};
use googletest::prelude::*;
use http::{StatusCode, header::LOCATION};
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{CreateEntityMap, DeleteEntityMap},
    },
    model::{Entity, NgsiLdMediaType},
    query::builders::SelectorTypeBuilder,
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};

/// Pins: Create `EntityMap` for Query Entities and Delete `EntityMap` with NGSILD-Snapshot naming an existing Snapshot
/// answer 422 `OperationNotSupported` with the same header and a detail naming the core, temporal and
/// entity-subscription operations; the refused create answers no Location and the map the delete names remains
/// Clause: 5.5.15, 6.3.22, 5.14.3, 5.14.4
/// Recorded: clause 5.5.15 makes every operation on a Snapshot local and permits the operations of the Core and
/// Temporal APIs alone, and an `EntityMap` serves only distributed operations; the clause names no error for another
/// operation, and Andromeda answers `OperationNotSupported`
#[gtest]
#[tokio::test]
async fn o_snp_scp_8104_an_entity_map_operation_naming_a_snapshot_is_refused_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let _cleanup = Cleanup::of(&broker).entity(&vehicle_id);
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map_id = &created.body().id;
    let selector = SelectorTypeBuilder::from(&vehicle_type).build()?;
    let on_snapshot = broker.on_snapshot(&snapshot_id);
    let mapped = broker
        .send_unchecked(on_snapshot.unchecked(&CreateEntityMap::builder().entity_type(&selector).build())?)
        .await;
    expect_that!(&mapped, refused_on_snapshot(&snapshot_id).without_header(LOCATION));
    let deleted = broker.send_unchecked(on_snapshot.unchecked(&DeleteEntityMap { id: map_id })?).await;
    expect_that!(&deleted, refused_on_snapshot(&snapshot_id));
    let remaining = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(map_id)
        .send()
        .await?;
    expect_that!(remaining.entity_map.get(&vehicle_id), some(anything()));
    broker.entity_maps().delete(map_id).send().await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
