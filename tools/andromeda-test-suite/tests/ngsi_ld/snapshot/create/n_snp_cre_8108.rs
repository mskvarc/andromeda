//! A temporal capture retrieves every page of its query.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Input, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query::{self, TemporalQuery},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot whose temporal query selects the Temporal Evolution of 251 Entities captures all of them: Query
/// Temporal Evolution under it with limit 250 answers 250 and then 1 Entity, each page with NGSILD-Results-Count
/// 251
/// Clause: 5.16.1.4, 5.7.4.4, 5.5.9, 6.3.13
#[gtest]
#[tokio::test]
async fn n_snp_cre_8108_a_temporal_capture_retrieves_every_page_of_its_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let evolutions = (0..=250)
        .map(|n| -> Result<TemporalEntity<Input>> {
            Ok(TemporalEntity::builder()
                .id(namespace.urn("Vehicle", format!("{n:04}"))?)
                .entity_type(vehicle_type.clone())
                .property("speed", Property::builder(n).observed_at("2025-01-01T00:00:00Z".parse()?))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    for evolution in &evolutions {
        broker.temporal().create_or_update(evolution).send().await?.created()?;
    }
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?)
        .temporal_query(
            QueryDocument::temporal_builder(TemporalQuery::after("2024-01-01T00:00:00Z".parse()?))
                .entities([vehicle_type.as_str()])
                .build()?,
        )
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
        .temporal()
        .query(temporal_query::after("2024-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .limit(250)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(251)));
    expect_that!(page.items(), len(eq(250)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .temporal()
        .query(temporal_query::after("2024-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .limit(250)
        .offset(250)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(251)));
    expect_that!(page_2.items(), len(eq(1)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
