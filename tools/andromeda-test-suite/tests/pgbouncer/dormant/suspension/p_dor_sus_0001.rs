//! Suspending a pooled tenant disconnects its pooler clients before the answer.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    namespace::Namespace,
    pooler_console::PoolerConsole,
    problem::answered,
    tenant_management::{Activity, database_of, set_activity},
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use http::StatusCode;

/// Pins: a tenant just read through a broker behind a transaction pooler holds at least one pooler client connection;
/// Suspending it answers 204, and by then the pooler lists no client connection of its database
/// Clause: 5.5.10
/// Tenant: private (the subject is a whole tenant's runtime, released by the suspension)
/// Recorded: Andromeda extension, not v1.9.1: tenant management (the /tenants resources), which clause 5.5.10 leaves
/// implementation-specific
#[gtest]
#[tokio::test]
async fn p_dor_sus_0001_suspending_a_pooled_tenant_disconnects_its_pooler_clients_before_the_answer() -> Result<()> {
    let namespace = Namespace::fresh();
    let pooled = broker::client(BrokerRole::PooledDurable)?;
    let set = TenantSet::provision(&pooled, &namespace, 1, 1).await?;
    let member = set.member(0);
    expect_that!(set.read(0).await.is_success(), eq(true));
    let database = database_of(&pooled, &member.tenant).await?;
    let console = PoolerConsole::of(BrokerRole::PooledDurable)?;
    expect_that!(console.clients_of(&database).await?, gt(0));
    let suspended = set_activity(&pooled, &member.tenant, Activity::Suspended).await?;
    expect_that!(&suspended, answered(StatusCode::NO_CONTENT));
    verify_that!(console.clients_of(&database).await?, eq(0))
}
