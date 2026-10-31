//! A query under a snapshot applies q geo and scope constraints.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::{geoquery, q, scope_query::ScopeQuery},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: Query Entities with NGSILD-Snapshot answers the Entity created under the Snapshot for its type alone and
/// under a q, a near geoquery and a `scopeQ` it meets, and answers nothing under a q, a geoquery and a `scopeQ` it
/// does not meet
/// Clause: 5.5.15, 5.7.2.4, 4.9, 4.10, 4.19
#[gtest]
#[tokio::test]
async fn n_snp_qry_8105_a_query_under_a_snapshot_applies_q_geo_and_scope_constraints() -> Result<()> {
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
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .scope(["/Fleet/West"])
        .property("speed", 30)
        .geo_property("location", Shape::Point(Position::try_new(13.0, 46.0)?))
        .build()?;
    broker.on_snapshot(&snapshot_id).entities().create(&vehicle).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(q::attr("speed").gt(20))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(q::attr("speed").gt(40))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .geo_q(geoquery::near_max(1000, Shape::Point(Position::try_new(13.0, 46.0)?)))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .geo_q(geoquery::near_max(1000, Shape::Point(Position::try_new(14.0, 47.0)?)))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(0)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .scope_q("/Fleet/West".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(1)));
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .scope_q("/Fleet/East".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page_7.items(), len(eq(0)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
