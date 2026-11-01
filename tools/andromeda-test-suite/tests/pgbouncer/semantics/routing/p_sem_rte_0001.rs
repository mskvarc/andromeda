//! A tenant created at runtime is routed by the pooler without a reload.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    namespace::Namespace,
    pooler_console::PoolerConsole,
    tenant_management::database_of,
    tenant_set::TenantSet,
};
use googletest::prelude::*;

/// Pins: a first write into a new tenant through a broker behind a transaction pooler creates it (201), its Device is
/// then retrieved with a success, and the pooler, never reloaded, holds a pool for the tenant's database
/// Clause: 5.5.10, 5.6.1, 5.7.1
/// Tenant: private (the subject is the implicit creation of a tenant)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`
/// routing every database by one wildcard entry
#[gtest]
#[tokio::test]
async fn p_sem_rte_0001_a_tenant_created_at_runtime_is_routed_by_the_pooler_without_a_reload() -> Result<()> {
    let namespace = Namespace::fresh();
    let pooled = broker::client(BrokerRole::PooledDurable)?;
    let set = TenantSet::provision(&pooled, &namespace, 1, 1).await?;
    expect_that!(set.read(0).await.is_success(), eq(true));
    let database = database_of(&pooled, &set.member(0).tenant).await?;
    let pool = PoolerConsole::of(BrokerRole::PooledDurable)?.pool_of(&database).await?;
    verify_that!(pool, some(anything()))
}
