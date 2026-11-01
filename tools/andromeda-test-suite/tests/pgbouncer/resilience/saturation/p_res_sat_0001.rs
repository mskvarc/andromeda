//! A saturated pool is answered 503 with a retry delay, never 500, and leaves other tenants unaffected.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    load_outcome::LoadOutcome,
    namespace::Namespace,
    pool_hold::PoolHold,
    pooler_console::PoolerConsole,
    tenant_management::database_of,
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use std::time::Duration;

/// The sessions that hold a database's whole pool: the pooler's `default_pool_size` of 8 and its
/// `reserve_pool_size` of 2.
const POOL: u8 = 10;

/// How long the pool is held.
const HELD_FOR: Duration = Duration::from_secs(20);

/// The reads sent to the saturated tenant, one after another.
const READS: usize = 64;

/// Pins: while every server connection of one tenant's database is held by a 20 s sleep, 64 Retrieve Entity of its
/// Device are each answered, with a success or 503 naming a delay in `Retry-After`, and a Retrieve Entity in another
/// tenant is answered with a success
/// Clause: 5.5.6, 6.3.2
/// Tenant: private (the subject is the pool of a whole tenant database, which the pooler addresses by database)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`;
/// the delay a 503 names follows IETF RFC 9110 section 10.2.3
#[gtest]
#[tokio::test]
async fn p_res_sat_0001_a_saturated_pool_is_answered_503_with_a_retry_delay_never_500() -> Result<()> {
    let namespace = Namespace::fresh();
    let resilience = broker::client(BrokerRole::Resilience)?;
    let set = TenantSet::provision(&resilience, &namespace, 2, 2).await?;
    let target = database_of(&resilience, &set.member(0).tenant).await?;
    let console = PoolerConsole::of(BrokerRole::Resilience)?;
    let hold = PoolHold::seize(&console, &target, POOL, HELD_FOR).await?;
    let bystander = set.read(1).await;
    let mut reads = Vec::with_capacity(READS);
    for _ in 0..READS {
        reads.push(set.read(0).await);
    }
    hold.release().await?;
    expect_that!(bystander.is_success(), eq(true));
    verify_that!(reads, each(predicate(|read: &LoadOutcome| read.is_success() || read.is_paced_unavailability())))
}
