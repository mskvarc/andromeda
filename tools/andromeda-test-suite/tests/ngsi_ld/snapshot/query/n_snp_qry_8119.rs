//! Temporal retrieval and query under a snapshot read that snapshot alone.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    eventually::retried,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Temporal Evolution created with NGSILD-Snapshot naming one Snapshot is retrieved and queried under it,
/// and is not found under a second Snapshot
/// Clause: 5.5.15, 5.7.3.4, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_snp_qry_8119_temporal_retrieval_and_query_under_a_snapshot_read_that_snapshot_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_2_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
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
    let snapshot_3 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot_3).send().await?;
    retried(async || {
        let snapshot_4 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_2_id)
            .send()
            .await?;
        verify_that!(snapshot_4.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(1).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker
        .on_snapshot(&snapshot_id)
        .temporal()
        .create_or_update(&vehicle_evolution)
        .send()
        .await?
        .created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", nth(0, any_attribute().value(1))));
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_2_id)
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .temporal()
        .query(temporal_query::between("2024-12-31T00:00:00Z".parse()?, "2025-01-02T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_2_id)
        .temporal()
        .query(temporal_query::between("2024-12-31T00:00:00Z".parse()?, "2025-01-02T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    broker.snapshots().delete(&snapshot_2_id).send().await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
