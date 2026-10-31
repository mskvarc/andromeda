//! A snapshot ends at its expiry in its own tenant alone.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    eventually::retried,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Duration, ErrorType, NgsiLdMediaType, Representation},
    snapshot::{QueryDocument, Snapshot, SnapshotId},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Snapshot of lifetime PT2S in a named Tenant is gone once it expires, together with the Subscription
/// created under it, while the Snapshot of the same id and a lifetime of one hour in the default Tenant remains
/// Clause: 5.16.1.4, Table 5.2.41-2, 5.5.15, 4.14
/// Tenant: private (the subject is the expiry of one Snapshot beside its namesake in another Tenant)
/// Timing: the named Tenant's Snapshot lives PT10S, so the Subscription created under it still finds it when the broker
/// answers slowly; the expiry wait covers 30 s
#[gtest]
#[tokio::test]
async fn n_snp_del_8101_a_snapshot_ends_at_its_expiry_in_its_own_tenant_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let subscription_id = namespace.subscription_id(1)?;
    let short = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime("PT10S".parse::<Duration>()?)
        .build()?;
    tenant.snapshots().create(&short).send().await?;
    let long = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .lifetime("PT1H".parse::<Duration>()?)
        .build()?;
    broker.snapshots().create(&long).send().await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(config.unreachable_origin().join("notify")?.as_str())
                    .accept(NgsiLdMediaType::Json)
                    .build()?,
            )
            .format(Representation::Normalized)
            .build()?,
        )
        .build()?;
    tenant.on_snapshot(&snapshot_id).subscriptions().create(&subscription).send().await?;
    retried(async || {
        let gone = tenant
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await;
        verify_that!(&gone, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound))?;
        Ok(())
    })
    .await?;
    let orphan = tenant
        .with_responses()
        .with_accept(Accept::any())
        .on_snapshot(&snapshot_id)
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&orphan, answered(StatusCode::NOT_FOUND));
    let remaining = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    expect_that!(remaining.id(), eq(&snapshot_id));
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
