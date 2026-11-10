//! A tenant over its snapshot quota loses its lowest priority snapshot first.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    namespace::Namespace,
    problem::answered,
    ranked_snapshot::{ranked_id, ranked_snapshot},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::headers::Accept;

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
    let snapshot_low_id = ranked_id(&namespace, "low")?;
    let snapshot_early_id = ranked_id(&namespace, "early")?;
    let snapshot_late_id = ranked_id(&namespace, "late")?;
    let snapshot_high_id = ranked_id(&namespace, "high")?;
    let snapshot_fifth_id = ranked_id(&namespace, "fifth")?;
    let snapshot = ranked_snapshot(&namespace, &snapshot_low_id, 1, "PT10H".parse()?)?;
    broker_snapshot_tenant.snapshots().create(&snapshot).send().await?;
    let snapshot_2 = ranked_snapshot(&namespace, &snapshot_early_id, 5, "PT1H".parse()?)?;
    broker_snapshot_tenant.snapshots().create(&snapshot_2).send().await?;
    let snapshot_3 = ranked_snapshot(&namespace, &snapshot_late_id, 5, "PT2H".parse()?)?;
    broker_snapshot_tenant.snapshots().create(&snapshot_3).send().await?;
    let snapshot_4 = ranked_snapshot(&namespace, &snapshot_high_id, 10, "PT1H".parse()?)?;
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
    let snapshot_5 = ranked_snapshot(&namespace, &snapshot_fifth_id, 5, "PT3H".parse()?)?;
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
