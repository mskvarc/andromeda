//! A capture retrieves every page of its query.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, Input, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot whose query selects 1,025 Entities captures all of them: Query Entities under it with limit
/// 1000 answers 1,000 and then 25 Entities, each page with NGSILD-Results-Count 1025
/// Clause: 5.16.1.4, 5.5.9, 6.3.13
#[gtest]
#[tokio::test]
async fn n_snp_cre_8106_a_capture_retrieves_every_page_of_its_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let entities = (0..=99)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", format!("{n:04}"))?)
                .entity_type(vehicle_type.clone())
                .property("speed", n)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    let entities_2 = (100..=199)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", format!("{n:04}"))?)
                .entity_type(vehicle_type.clone())
                .property("speed", n)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities_2).send().await?.done()?;
    let entities_3 = (200..=299)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", format!("{n:04}"))?)
                .entity_type(vehicle_type.clone())
                .property("speed", n)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities_3).send().await?.done()?;
    let entities_4 = (300..=399)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", format!("{n:04}"))?)
                .entity_type(vehicle_type.clone())
                .property("speed", n)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities_4).send().await?.done()?;
    let entities_5 = (400..=499)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", format!("{n:04}"))?)
                .entity_type(vehicle_type.clone())
                .property("speed", n)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities_5).send().await?.done()?;
    let entities_6 = (500..=599)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", format!("{n:04}"))?)
                .entity_type(vehicle_type.clone())
                .property("speed", n)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities_6).send().await?.done()?;
    let entities_7 = (600..=699)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", format!("{n:04}"))?)
                .entity_type(vehicle_type.clone())
                .property("speed", n)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities_7).send().await?.done()?;
    let entities_8 = (700..=799)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", format!("{n:04}"))?)
                .entity_type(vehicle_type.clone())
                .property("speed", n)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities_8).send().await?.done()?;
    let entities_9 = (800..=899)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", format!("{n:04}"))?)
                .entity_type(vehicle_type.clone())
                .property("speed", n)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities_9).send().await?.done()?;
    let entities_10 = (900..=999)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", format!("{n:04}"))?)
                .entity_type(vehicle_type.clone())
                .property("speed", n)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities_10).send().await?.done()?;
    let entities_11 = (1000..=1024)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", format!("{n:04}"))?)
                .entity_type(vehicle_type.clone())
                .property("speed", n)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities_11).send().await?.done()?;
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
        verify_that!(snapshot_2.status(), eq("success".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .limit(1000)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(1025)));
    expect_that!(page.items(), len(eq(1000)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .limit(1000)
        .offset(1000)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(1025)));
    expect_that!(page_2.items(), len(eq(25)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
