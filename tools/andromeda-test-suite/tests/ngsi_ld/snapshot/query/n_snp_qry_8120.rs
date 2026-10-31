//! A temporal query under a snapshot aggregates pages and counts.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    eventually::retried,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{RepresentationSpelling, ResultsCount},
    },
    model::{
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: Query Temporal Evolution with NGSILD-Snapshot, options `aggregatedValues`, sum over one-hour periods,
/// limit 1, offset 1 and count true answers one Entity, NGSILD-Results-Count 2, and a sum per period
/// Clause: 5.5.15, 5.7.4.4, 4.5.19, 5.5.9, 6.3.13
#[gtest]
#[tokio::test]
async fn n_snp_qry_8120_a_temporal_query_under_a_snapshot_aggregates_pages_and_counts() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let captured = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(captured.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    for suffix in ["a", "b"] {
        let vehicle = TemporalEntity::builder()
            .id(namespace.urn("Vehicle", suffix)?)
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(10).observed_at("2025-01-01T00:00:00Z".parse()?))
            .property("speed", Property::builder(20).observed_at("2025-01-01T01:00:00Z".parse()?))
            .build()?;
        broker.on_snapshot(&snapshot_id).temporal().create_or_update(&vehicle).send().await?.created()?;
    }
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&snapshot_id)
        .temporal()
        .query_aggregated(
            temporal_query::between("2025-01-01T00:00:00Z".parse()?, "2025-01-01T02:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Sum])
                .aggregation_period("PT1H".parse()?),
        )
        .entity_type(&vehicle_type)
        .attrs(vec!["speed".parse()?])
        .spelling(RepresentationSpelling::default().in_options())
        .limit(1)
        .offset(1)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::OK));
    let page = headed.outcome;
    expect_that!(page.total(), some(eq(2)));
    expect_that!(page.items(), len(eq(1)));
    let aggregated = page.items().first().or_fail()?;
    expect_that!(
        aggregated,
        has_aggregate(
            "speed",
            AggregationMethod::Sum,
            all!(len(eq(2)), nth(0, period().value(10)), nth(1, period().value(20)))
        )
    );
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
