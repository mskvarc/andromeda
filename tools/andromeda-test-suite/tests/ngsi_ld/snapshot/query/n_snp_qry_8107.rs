//! A query under a snapshot orders pages and counts.

use andromeda_test_suite::{broker, eventually::retried, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, NgsiLdMediaType},
    query::ordering::OrderBy,
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: Query Entities with NGSILD-Snapshot, `orderBy` speed ascending, limit 1, offset 1 and count true answers
/// the Entity of the middle speed and NGSILD-Results-Count 3
/// Clause: 5.5.15, 4.23, 5.5.9, 6.3.13
#[gtest]
#[tokio::test]
async fn n_snp_qry_8107_a_query_under_a_snapshot_orders_pages_and_counts() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_a_id = namespace.urn("Vehicle", "a")?;
    let vehicle_b_id = namespace.urn("Vehicle", "b")?;
    let vehicle_c_id = namespace.urn("Vehicle", "c")?;
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
    let vehicle_a = Entity::builder()
        .id(vehicle_a_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 3)
        .build()?;
    broker.on_snapshot(&snapshot_id).entities().create(&vehicle_a).send().await?.done()?;
    let vehicle_b = Entity::builder()
        .id(vehicle_b_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.on_snapshot(&snapshot_id).entities().create(&vehicle_b).send().await?.done()?;
    let vehicle_c = Entity::builder()
        .id(vehicle_c_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 2)
        .build()?;
    broker.on_snapshot(&snapshot_id).entities().create(&vehicle_c).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .ordering("speed;asc".parse::<OrderBy>()?)
        .limit(1)
        .offset(1)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(3)));
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_c_id)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
