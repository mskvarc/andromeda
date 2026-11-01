//! Server backends terminated under load are replaced.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    load_run::run_load,
    namespace::Namespace,
    server_backends::ServerBackends,
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

/// When the backends are terminated, after the load started.
const DISRUPT_AFTER: Duration = Duration::from_secs(3);

/// Pins: while 24 workers retrieve the Devices of 16 tenants for 10 s through a broker behind a transaction pooler,
/// every server backend on those tenants' databases is terminated 3 s in (at least one is); afterwards each tenant's
/// Device is retrieved with a success
/// Clause: 5.7.1
/// Tenant: private (the subject is the backends of whole tenant databases)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`
#[gtest]
#[tokio::test]
async fn p_res_chn_0003_server_backends_terminated_under_load_are_replaced() -> Result<()> {
    let namespace = Namespace::fresh();
    let resilience = broker::quiet_client(BrokerRole::Resilience)?;
    let set = TenantSet::provision(&resilience, &namespace, TENANTS, TENANTS).await?;
    let databases = databases_of(&resilience, set.members().map(|member| &member.tenant)).await?;
    let server = ServerBackends::of(BrokerRole::Resilience)?;
    let picker = UniformPicker::new(set.len(), 99)?;
    let disruption = async {
        sleep(DISRUPT_AFTER).await;
        server.terminate(&databases).await
    };
    let (report, terminated) = tokio::join!(run_load(WORKERS, LOAD, |_| set.read(picker.pick())), disruption);
    println!("{report}");
    expect_that!(terminated?, gt(0));
    for index in 0..set.len() {
        expect_that!(set.read(index).await.is_success(), eq(true), "tenant {index}");
    }
    Ok(())
}
