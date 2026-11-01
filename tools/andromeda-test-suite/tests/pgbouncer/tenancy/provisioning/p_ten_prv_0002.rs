//! First writes into new tenants behind a tight transaction pooler each create their tenant while the wake
//! reconciliation pass probes them, and every pooled session of each tenant carries the broker's database defaults.

use andromeda_postgresql_seaquery::setting::Setting;
use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    fresh_tenant_writes::expect_first_writes_create_their_tenants,
    namespace::Namespace,
    pooled_setting::pooled_flag,
    pooler_console::PoolerConsole,
    tenant_management::database_of,
};
use googletest::prelude::*;

/// The first writes sent at once, each into a tenant of its own.
const PARALLEL_FIRST_WRITES: usize = 24;

/// Pins: 24 Create Entity requests sent at once through a broker behind a transaction pooler holding two servers per
/// database and keeping an idle one five seconds, each naming a tenant that does not exist yet, each answer 201 while
/// the wake reconciliation pass probes the tenants it has never probed, each tenant then holds exactly the Entity
/// written into it, and a session the pooler serves for each tenant's database has
/// `timescaledb.enable_chunk_skipping` on
/// Clause: 5.5.10, 5.6.1, 6.3.2
/// Tenant: private (the subject is the implicit creation of new tenants by their first write)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`
/// and installs its session defaults on a tenant database before any pooled session opens on it
#[gtest]
#[tokio::test]
async fn p_ten_prv_0002_first_writes_into_new_tenants_behind_a_tight_pooler_carry_the_database_defaults() -> Result<()> {
    let namespace = Namespace::fresh();
    let first_write = broker::client(BrokerRole::FirstWrite)?;
    expect_first_writes_create_their_tenants(&first_write, &namespace, PARALLEL_FIRST_WRITES).await?;
    let console = PoolerConsole::of(BrokerRole::FirstWrite)?;
    for index in 0..PARALLEL_FIRST_WRITES {
        let database = database_of(&first_write, &namespace.numbered_tenant(index)?).await?;
        expect_that!(
            pooled_flag(&console, &database, Setting::TimescaledbEnableChunkSkipping).await?,
            eq(true),
            "tenant database {database}"
        );
    }
    Ok(())
}
