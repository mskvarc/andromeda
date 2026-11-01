//! Reconnecting the pooler's servers under load loses no request.

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
const LOAD: Duration = Duration::from_secs(10);

/// When the reconnect is sent, after the load started.
const DISRUPT_AFTER: Duration = Duration::from_secs(3);

/// Pins: 24 workers retrieving the Devices of 16 tenants for 10 s through a broker behind a transaction pooler, whose
/// pooler is told to RECONNECT every server of those tenants' databases 3 s in, have every request answered with a
/// success
/// Clause: 5.7.1
/// Tenant: private (the subject is the pools of whole tenant databases, which the pooler addresses by database)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`
#[gtest]
#[tokio::test]
async fn p_res_chn_0001_reconnecting_the_poolers_servers_under_load_loses_no_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let resilience = broker::quiet_client(BrokerRole::Resilience)?;
    let set = TenantSet::provision(&resilience, &namespace, TENANTS, TENANTS).await?;
    let databases = databases_of(&resilience, set.members().map(|member| &member.tenant)).await?;
    let console = PoolerConsole::of(BrokerRole::Resilience)?;
    let picker = UniformPicker::new(set.len(), 99)?;
    let disruption = async {
        sleep(DISRUPT_AFTER).await;
        for database in &databases {
            console.reconnect(database).await?;
        }
        Ok::<(), PoolerConsoleError>(())
    };
    let (report, disrupted) = tokio::join!(run_load(WORKERS, LOAD, |_| set.read(picker.pick())), disruption);
    disrupted?;
    println!("{report}");
    verify_that!(report.unsuccessful(), eq(0))
}
