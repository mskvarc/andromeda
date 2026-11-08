//! A tenant over its snapshot quota loses its lowest priority snapshot first.

use andromeda_test_suite::{broker, config::BrokerRole, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    snapshot::{QueryDocument, Snapshot, SnapshotId},
};

/// Pins: On a broker holding at most three Snapshots per Tenant, a fourth Snapshot evicts the one of
/// `snapshotPriority` 1, and a fifth then evicts, of the three of `snapshotPriority` 5, the one expiring first;
/// every other Snapshot remains
/// Clause: 5.5.15, Table 5.2.41-1
/// Tenant: private (the quota counts every Snapshot of the Tenant)
#[gtest]
#[tokio::test]
async fn a_snp_lim_8140_a_tenant_over_its_snapshot_quota_loses_its_lowest_priority_snapshot_first() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_snapshot = broker::client(BrokerRole::Durable)?;
    let broker_snapshot_tenant = broker::private_tenant(&broker_snapshot, &namespace).await?;
    let snapshot_low_id = format!("urn:ngsi-ld:Snapshot:{namespace}-low").parse::<SnapshotId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_early_id = format!("urn:ngsi-ld:Snapshot:{namespace}-early").parse::<SnapshotId>()?;
    let snapshot_late_id = format!("urn:ngsi-ld:Snapshot:{namespace}-late").parse::<SnapshotId>()?;
    let snapshot_high_id = format!("urn:ngsi-ld:Snapshot:{namespace}-high").parse::<SnapshotId>()?;
    let snapshot_fifth_id = format!("urn:ngsi-ld:Snapshot:{namespace}-fifth").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-low").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime("PT10H".parse()?)
        .priority(1)
        .build()?;
    broker_snapshot_tenant.snapshots().create(&snapshot).send().await?;
    let snapshot_2 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-early").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime("PT1H".parse()?)
        .priority(5)
        .build()?;
    broker_snapshot_tenant.snapshots().create(&snapshot_2).send().await?;
    let snapshot_3 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-late").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime("PT2H".parse()?)
        .priority(5)
        .build()?;
    broker_snapshot_tenant.snapshots().create(&snapshot_3).send().await?;
    let snapshot_4 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-high").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime("PT1H".parse()?)
        .priority(10)
        .build()?;
    broker_snapshot_tenant.snapshots().create(&snapshot_4).send().await?;
    let answer = broker_snapshot_tenant
        .with_responses()
        .with_accept(Accept::any())
        .snapshots()
        .retrieve(&snapshot_low_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    broker_snapshot_tenant
        .with_accept(Accept::any())
        .snapshots()
        .retrieve(&snapshot_early_id)
        .send()
        .await?;
    broker_snapshot_tenant
        .with_accept(Accept::any())
        .snapshots()
        .retrieve(&snapshot_late_id)
        .send()
        .await?;
    broker_snapshot_tenant
        .with_accept(Accept::any())
        .snapshots()
        .retrieve(&snapshot_high_id)
        .send()
        .await?;
    let snapshot_5 = Snapshot::builder()
        .id(format!("urn:ngsi-ld:Snapshot:{namespace}-fifth").parse::<SnapshotId>()?)
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime("PT3H".parse()?)
        .priority(5)
        .build()?;
    broker_snapshot_tenant.snapshots().create(&snapshot_5).send().await?;
    let answer_2 = broker_snapshot_tenant
        .with_responses()
        .with_accept(Accept::any())
        .snapshots()
        .retrieve(&snapshot_early_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    broker_snapshot_tenant
        .with_accept(Accept::any())
        .snapshots()
        .retrieve(&snapshot_fifth_id)
        .send()
        .await?;
    broker_snapshot_tenant
        .with_accept(Accept::any())
        .snapshots()
        .retrieve(&snapshot_late_id)
        .send()
        .await?;
    broker_snapshot_tenant
        .with_accept(Accept::any())
        .snapshots()
        .retrieve(&snapshot_high_id)
        .send()
        .await?;
    broker_snapshot_tenant.snapshots().delete(&snapshot_fifth_id).send().await?;
    broker_snapshot_tenant.snapshots().delete(&snapshot_late_id).send().await?;
    broker_snapshot_tenant.snapshots().delete(&snapshot_high_id).send().await?;
    Ok(())
}
