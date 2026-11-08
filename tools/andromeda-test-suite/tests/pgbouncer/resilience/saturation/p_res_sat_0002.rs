//! A name lookup on a saturated pool is answered 503 with a retry delay, never 500.

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
use futures::join;
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::ResultsCount,
    query::{q, temporal_query},
};
use std::time::Duration;

/// The sessions that hold a database's whole pool: the pooler's `default_pool_size` of 8 and its
/// `reserve_pool_size` of 2.
const POOL: u8 = 10;

/// How long the pool is held: past the pooler's `query_wait_timeout` of 15 s, so a statement queued behind it is shed.
const HELD_FOR: Duration = Duration::from_secs(20);

/// Pins: while every server connection of one tenant's database is held by a 20 s sleep, five queries of an Entity
/// type the tenant never held (Query Entities, with `q`, with `count`, Query Temporal Evolution of Entities, with
/// `q`), sent at once, are each answered with a success or 503 naming a delay in `Retry-After`, and at least one is
/// answered 503
/// Clause: 5.5.6, 6.3.2
/// Tenant: private (the subject is the pool of a whole tenant database, which the pooler addresses by database)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`
/// and resolves a name it has not cached against the tenant's database before the query runs
#[gtest]
#[tokio::test]
async fn p_res_sat_0002_a_name_lookup_on_a_saturated_pool_is_answered_503_with_a_retry_delay_never_500() -> Result<()> {
    let namespace = Namespace::fresh();
    let resilience = broker::client(BrokerRole::Resilience)?;
    let set = TenantSet::provision(&resilience, &namespace, 1, 1).await?;
    let tenant = set.member(0).client.with_responses();
    let target = database_of(&resilience, &set.member(0).tenant).await?;
    let never_held = namespace.entity_type("NeverHeld")?;
    let never_written = q::attr("neverWritten").eq(1).build()?;
    let never_written_temporal = never_written.clone();
    let long_ago = "2100-01-01T00:00:00Z".parse()?;
    let console = PoolerConsole::of(BrokerRole::Resilience)?;
    let hold = PoolHold::seize(&console, &target, POOL, HELD_FOR).await?;
    let (plain, filtered, counted, temporal, temporal_filtered) = join!(
        async { LoadOutcome::of(&tenant.entities().query().entity_type(&never_held).send().await) },
        async { LoadOutcome::of(&tenant.entities().query().entity_type(&never_held).q(never_written).send().await,) },
        async { LoadOutcome::of(&tenant.entities().query().entity_type(&never_held).count(ResultsCount::Include).send().await,) },
        async { LoadOutcome::of(&tenant.temporal().query(temporal_query::before(long_ago)).entity_type(&never_held).send().await,) },
        async {
            LoadOutcome::of(
                &tenant
                    .temporal()
                    .query(temporal_query::before(long_ago))
                    .entity_type(&never_held)
                    .q(never_written_temporal)
                    .send()
                    .await,
            )
        },
    );
    hold.release().await?;
    let answers = vec![plain, filtered, counted, temporal, temporal_filtered];
    println!("{answers:?}");
    expect_that!(
        answers,
        each(predicate(|answer: &LoadOutcome| answer.is_success() || answer.is_paced_unavailability()))
    );
    verify_that!(answers, contains(predicate(LoadOutcome::is_paced_unavailability)))
}
