//! A killed pool recovers through 503 answers.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    load_outcome::LoadOutcome,
    namespace::Namespace,
    pool_outage::kill_then_resume,
    pooler_console::PoolerConsole,
    tenant_management::database_of,
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use http::StatusCode;
use std::time::Duration;
use tokio::time::{Instant, sleep};

/// How long the tenant may take to serve again once its pool is resumed: the pooler keeps a killed pool's failed
/// login for its `server_login_retry` of 15 s.
const RECOVERY_BOUND: Duration = Duration::from_secs(15);

/// The pause between two reads.
const READ_SPACING: Duration = Duration::from_millis(500);

/// Pins: after the pooler KILLs a tenant's database pool and RESUMEs it a second later, Retrieve Entity of its Device,
/// sent every 500 ms, is answered with a success within 15 s, and every answer before it is 503 or none, never another
/// error
/// Clause: 5.5.6, 6.3.2
/// Tenant: private (the subject is the pool of a whole tenant database, which the pooler addresses by database)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`
#[gtest]
#[tokio::test]
async fn p_res_out_0001_a_killed_pool_recovers_through_503_answers() -> Result<()> {
    let namespace = Namespace::fresh();
    let resilience = broker::client(BrokerRole::Resilience)?;
    let set = TenantSet::provision(&resilience, &namespace, 1, 1).await?;
    let database = database_of(&resilience, &set.member(0).tenant).await?;
    kill_then_resume(&PoolerConsole::of(BrokerRole::Resilience)?, &database).await?;
    let deadline = Instant::now() + RECOVERY_BOUND;
    let mut before_recovery = Vec::new();
    let recovered = loop {
        let read = set.read(0).await;
        if read.is_success() {
            break true;
        }
        before_recovery.push(read);
        if Instant::now() >= deadline {
            break false;
        }
        sleep(READ_SPACING).await;
    };
    expect_that!(
        before_recovery,
        each(predicate(|read: &LoadOutcome| matches!(
            read.status(),
            Some(StatusCode::SERVICE_UNAVAILABLE) | None
        )))
    );
    verify_that!(recovered, eq(true))
}
