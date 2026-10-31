//! Batch create update and delete under a snapshot change the snapshot alone.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    eventually::retried,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: Batch Entity Creation, Update and Deletion with NGSILD-Snapshot answer 201, 204 and 204, and Query
/// Entities under the Snapshot then answers the one remaining Entity with its updated value while no Entity exists
/// live
/// Clause: 5.5.15, 5.6.7.4, 5.6.9.4, 5.6.10.4
#[gtest]
#[tokio::test]
async fn n_snp_qry_8103_batch_create_update_and_delete_under_a_snapshot_change_the_snapshot_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
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
    let entities = [
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 1)
            .build()?,
        Entity::builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 2)
            .build()?,
    ];
    broker.on_snapshot(&snapshot_id).entities().batch_create(&entities).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(vehicle_id.clone(), vehicle_type.clone())
        .property("speed", 11)
        .build()?];
    broker.on_snapshot(&snapshot_id).entities().batch_update(&fragments).send().await?.done()?;
    let ids = [vehicle_2_id.clone()];
    broker.on_snapshot(&snapshot_id).entities().batch_delete(&ids).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(0, all!(has_id(&vehicle_id), has_attribute("speed", any_attribute().value(11))))
    );
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
