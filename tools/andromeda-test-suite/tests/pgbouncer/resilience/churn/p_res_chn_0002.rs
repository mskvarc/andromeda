//! A paused pooler stalls requests, then serves them once resumed.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    load_run::run_load,
    namespace::Namespace,
    pooler_console::{PoolerConsole, PoolerConsoleError},
    tenant_management::databases_of,
    tenant_picker::UniformPicker,
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use std::time::Duration;
use tokio::time::sleep;

/// The tenants the load spreads over.
const TENANTS: usize = 16;

/// The workers sending requests at once.
const WORKERS: usize = 24;

/// How long the load runs.
const LOAD: Duration = Duration::from_secs(12);

/// When the pause is sent, after the load started.
const PAUSE_AFTER: Duration = Duration::from_secs(2);

/// How long the databases stay paused: well inside the pooler's `query_wait_timeout` of 15 s, so no stalled
/// transaction is shed.
const PAUSED_FOR: Duration = Duration::from_secs(3);

/// The slowest a request may be: the pause and a margin.
const SLOWEST: Duration = Duration::from_secs(8);

/// Pins: 24 workers retrieving the Devices of 16 tenants for 12 s through a broker behind a transaction pooler, whose
/// pooler PAUSEs those tenants' databases 2 s in and RESUMEs them 3 s later, have every request answered with a
/// success, none slower than 8 s
/// Clause: 5.7.1
/// Tenant: private (the subject is the pools of whole tenant databases, which the pooler addresses by database)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`
#[gtest]
#[tokio::test]
async fn p_res_chn_0002_a_paused_pooler_stalls_requests_then_serves_them_once_resumed() -> Result<()> {
    let namespace = Namespace::fresh();
    let resilience = broker::quiet_client(BrokerRole::Resilience)?;
    let set = TenantSet::provision(&resilience, &namespace, TENANTS, TENANTS).await?;
    let databases = databases_of(&resilience, set.members().map(|member| &member.tenant)).await?;
    let console = PoolerConsole::of(BrokerRole::Resilience)?;
    let picker = UniformPicker::new(set.len(), 99)?;
    let disruption = async {
        sleep(PAUSE_AFTER).await;
        for database in &databases {
            console.pause(database).await?;
        }
        sleep(PAUSED_FOR).await;
        for database in &databases {
            console.resume(database).await?;
        }
        Ok::<(), PoolerConsoleError>(())
    };
    let (report, disrupted) = tokio::join!(run_load(WORKERS, LOAD, |_| set.read(picker.pick())), disruption);
    disrupted?;
    println!("{report}");
    expect_that!(report.unsuccessful(), eq(0));
    verify_that!(report.slowest(), lt(SLOWEST))
}
