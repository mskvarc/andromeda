//! Two creates that each take a tenant over its snapshot quota evict the two snapshots first in order.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    namespace::Namespace,
    problem::answered,
    ranked_snapshot::{ranked_id, ranked_snapshot},
};
use futures::try_join;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::headers::Accept;

/// Pins: On a broker holding at most three Snapshots per Tenant, holding Snapshots of `snapshotPriority` 1, 2 and 10,
/// two Snapshots of `snapshotPriority` 10 created at once are both created, the Snapshots of `snapshotPriority` 1 and
/// 2 are evicted, and the other three remain
/// Clause: 5.5.15, Table 5.2.41-1
/// Tenant: private (the quota counts every Snapshot of the Tenant)
#[gtest]
#[tokio::test]
async fn a_snp_lim_8143_two_creates_that_each_take_a_tenant_over_its_snapshot_quota_evict_the_two_snapshots_first_in_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::client(BrokerRole::Durable)?, &namespace).await?;
    let lowest_id = ranked_id(&namespace, "lowest")?;
    let low_id = ranked_id(&namespace, "low")?;
    let high_id = ranked_id(&namespace, "high")?;
    let first_id = ranked_id(&namespace, "first")?;
    let second_id = ranked_id(&namespace, "second")?;
    tenant
        .snapshots()
        .create(&ranked_snapshot(&namespace, &lowest_id, 1, "PT1H".parse()?)?)
        .send()
        .await?;
    tenant
        .snapshots()
        .create(&ranked_snapshot(&namespace, &low_id, 2, "PT1H".parse()?)?)
        .send()
        .await?;
    tenant
        .snapshots()
        .create(&ranked_snapshot(&namespace, &high_id, 10, "PT1H".parse()?)?)
        .send()
        .await?;
    let first = ranked_snapshot(&namespace, &first_id, 10, "PT1H".parse()?)?;
    let second = ranked_snapshot(&namespace, &second_id, 10, "PT1H".parse()?)?;
    let snapshots = tenant.snapshots();
    try_join!(snapshots.create(&first).send(), snapshots.create(&second).send())?;
    for evicted_id in [&lowest_id, &low_id] {
        let evicted = tenant.with_responses().with_accept(Accept::any()).snapshots().retrieve(evicted_id).send().await;
        expect_that!(&evicted, answered(StatusCode::NOT_FOUND));
    }
    for kept in [&high_id, &first_id, &second_id] {
        tenant.with_accept(Accept::any()).snapshots().retrieve(kept).send().await?;
    }
    for kept in [&high_id, &first_id, &second_id] {
        tenant.snapshots().delete(kept).send().await?;
    }
    Ok(())
}
