//! A requested lifetime within the limit sets the expiry.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace};
use chrono::{DateTime as ChronoDateTime, Utc};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Duration, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotPriority, SnapshotStatus},
};

/// Pins: A Snapshot created with `snapshotLifetime` P10D and no `snapshotPriority` expires ten days from its
/// creation (nine whole days from now), has `snapshotPriority` 5, and carries `lastUsedAt` from its creation
/// Clause: 5.16.1.4, Table 5.2.41-1, Table 5.2.41-2
#[gtest]
#[tokio::test]
async fn n_snp_cre_8103_a_requested_lifetime_within_the_limit_sets_the_expiry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime("P10D".parse::<Duration>()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(retrieved.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        let expires_at = ChronoDateTime::parse_from_rfc3339(&retrieved.output().expires_at.to_string())?;
        let days = (expires_at.with_timezone(&Utc) - Utc::now()).num_days();
        verify_that!(days, ge(9))?;
        verify_that!(days, le(10))?;
        verify_that!(retrieved.priority(), some(eq(SnapshotPriority::try_new(5)?)))?;
        verify_that!(retrieved.output().last_used_at, some(anything()))?;
        verify_that!(retrieved.output().created_at, some(anything()))?;
        Ok(())
    })
    .await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
