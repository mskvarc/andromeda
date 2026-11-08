//! Large batch upserts commit whole across concurrent tenants.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::BrokerRole,
    entity_matcher::has_attribute,
    load_outcome::LoadOutcome,
    namespace::Namespace,
    tenant_management::{TenantView, read_tenant},
    tenant_set::TenantSet,
};
use futures::future::join_all;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// The tenants writing at once.
const TENANTS: usize = 24;

/// The Entities of each tenant's batch: past the size the broker stages a batch through a temporary relation at.
const BATCH: i32 = 500;

/// Pins: 24 tenants each sent one Batch Entity Upsert of 500 new Entities at the same moment, through a broker behind
/// a transaction pooler with two server connections per database, are each answered with a success; in every tenant
/// the last Entity reads back with its own `seq`, and the tenant's usage counts all 500 beside the Entity its first
/// write created
/// Clause: 5.6.8, 5.7.1
/// Tenant: private (the subject is whole tenants written at once, which the usage counts)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling
/// `PgBouncer`; the tenant usage (`options=usage` of the /tenants resources) counts the Entities a tenant holds
#[gtest]
#[tokio::test]
async fn p_sem_bat_0001_large_batch_upserts_commit_whole_across_concurrent_tenants() -> Result<()> {
    let namespace = Namespace::fresh();
    let pooled = broker::client(BrokerRole::PooledDurable)?;
    let set = TenantSet::provision(&pooled, &namespace, TENANTS, TENANTS).await?;
    let device_type = namespace.entity_type("Device")?;
    let batch = (0..BATCH)
        .map(|seq| {
            Ok(Entity::builder()
                .id(namespace.urn("Device", format!("atomic-{seq}"))?)
                .entity_type(device_type.clone())
                .property("seq", seq)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    let written = join_all(
        set.members()
            .map(|member| async { LoadOutcome::of(&member.client.with_responses().entities().batch_upsert(&batch).send().await) }),
    )
    .await;
    expect_that!(written, each(predicate(|answer: &LoadOutcome| answer.is_success())));
    let last = namespace.urn("Device", format!("atomic-{}", BATCH - 1))?;
    for member in set.members() {
        let retrieved = member
            .client
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&last)
            .send()
            .await?;
        expect_that!(retrieved.body(), has_attribute("seq", any_attribute().value(BATCH - 1)));
        let usage = read_tenant(&pooled, &member.tenant, TenantView::Usage).await?;
        expect_that!(usage.entity_count, some(eq(u64::try_from(BATCH)? + 1)));
    }
    Ok(())
}
