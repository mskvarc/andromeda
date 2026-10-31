//! A snapshot lives one hour by default and at most thirty days.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace};
use chrono::{DateTime as ChronoDateTime, Utc};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Duration, NgsiLdMediaType},
    snapshot::{QueryDocument, Snapshot, SnapshotFragment, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot created without `snapshotLifetime` reports PT1H and expires within the day; one created or
/// updated with PT1000H reports P30D and expires twenty-nine whole days from now
/// Clause: 5.16.1.4, 5.16.4.4, Table 5.2.41-1
/// Recorded: clause 5.16.1.4 sets `expiresAt` from the requested `snapshotLifetime` "applying the configured
/// limit"; Andromeda's default lifetime is one hour, its limit thirty days, and it reports the lifetime it applied
/// as `snapshotLifetime`
#[gtest]
#[tokio::test]
async fn o_snp_cre_8102_a_snapshot_lives_one_hour_by_default_and_at_most_thirty_days() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_1_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let snapshot_2_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    let days_from_now = |expires_at: &DateTime| -> Result<i64> {
        let parsed = ChronoDateTime::parse_from_rfc3339(&expires_at.to_string())?;
        Ok((parsed.with_timezone(&Utc) - Utc::now()).num_days())
    };
    let default_lifetime = Snapshot::builder()
        .id(snapshot_1_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&default_lifetime).send().await?;
    let created = retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_1_id)
            .send()
            .await?;
        verify_that!(retrieved.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        verify_that!(retrieved.lifetime().map(ToString::to_string), some(eq("PT1H")))?;
        verify_that!(days_from_now(&retrieved.output().expires_at)?, eq(0))?;
        Ok(retrieved.output().created_at)
    })
    .await?;
    let again = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_1_id)
        .send()
        .await?;
    expect_that!(again.output().created_at, some(anything()));
    let created_at = ChronoDateTime::parse_from_rfc3339(&created.map(|instant| instant.to_string()).unwrap_or_default())?;
    let expires_at = ChronoDateTime::parse_from_rfc3339(&again.output().expires_at.to_string())?;
    expect_that!(expires_at > created_at, eq(true));
    let long_lifetime = Snapshot::builder()
        .id(snapshot_2_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime("PT1000H".parse::<Duration>()?)
        .build()?;
    broker.snapshots().create(&long_lifetime).send().await?;
    retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_2_id)
            .send()
            .await?;
        verify_that!(retrieved.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        verify_that!(retrieved.lifetime().map(ToString::to_string), some(eq("P30D")))?;
        verify_that!(days_from_now(&retrieved.output().expires_at)?, ge(29))?;
        verify_that!(days_from_now(&retrieved.output().expires_at)?, le(30))?;
        Ok(())
    })
    .await?;
    let fragment = SnapshotFragment::builder().lifetime("PT1000H".parse::<Duration>()?).build()?;
    let updated = broker.snapshots().update(&snapshot_1_id, &fragment).send().await?;
    expect_that!(updated.lifetime().map(ToString::to_string), some(eq("P30D")));
    expect_that!(days_from_now(&updated.output().expires_at)?, ge(29));
    expect_that!(days_from_now(&updated.output().expires_at)?, le(30));
    broker.snapshots().delete(&snapshot_2_id).send().await?;
    broker.snapshots().delete(&snapshot_1_id).send().await?;
    Ok(())
}
