//! Suspending a tenant of a broker without a pooler closes its backends.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    eventually::{Poll, eventually},
    namespace::Namespace,
    problem::answered,
    server_backends::{BackendScope, ServerBackends},
    tenant_management::{Activity, database_of, set_activity},
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use http::StatusCode;
use std::time::Duration;

/// How long the server is given to reap a backend whose client closed: a backend exits after its client's close,
/// not with it.
const REAP_BOUND: Duration = Duration::from_secs(2);

/// Pins: a tenant just read through a broker reaching its PostgreSQL server directly holds at least one client
/// backend on its database; Suspending it answers 204, and within 2 s the server holds no client backend on it
/// Clause: 5.5.10
/// Tenant: private (the subject is a whole tenant's runtime, released by the suspension)
/// Recorded: Andromeda extension, not v1.9.1: tenant management (the /tenants resources), which clause 5.5.10 leaves
/// implementation-specific
#[gtest]
#[tokio::test]
async fn p_dor_sus_0002_suspending_a_tenant_of_a_broker_without_a_pooler_closes_its_backends() -> Result<()> {
    let namespace = Namespace::fresh();
    let direct = broker::client(BrokerRole::Direct)?;
    let set = TenantSet::provision(&direct, &namespace, 1, 1).await?;
    let member = set.member(0);
    expect_that!(set.read(0).await.is_success(), eq(true));
    let scope = BackendScope::Database(database_of(&direct, &member.tenant).await?);
    let server = ServerBackends::of(BrokerRole::Direct)?;
    expect_that!(server.count(&scope).await?, gt(0));
    let suspended = set_activity(&direct, &member.tenant, Activity::Suspended).await?;
    expect_that!(&suspended, answered(StatusCode::NO_CONTENT));
    eventually(REAP_BOUND, || async {
        match server.count(&scope).await {
            Ok(0) => Poll::Ready(Ok(())),
            Ok(held) => Poll::Pending(held),
            Err(error) => Poll::Ready(Err(error)),
        }
    })
    .await??;
    Ok(())
}
