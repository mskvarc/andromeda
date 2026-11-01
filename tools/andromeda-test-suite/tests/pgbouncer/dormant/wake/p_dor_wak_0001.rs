//! A tenant with nothing due stays dormant and holds no backend.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    eviction::evict_by_pressure,
    namespace::Namespace,
    server_backends::{BackendScope, ServerBackends},
    tenant_management::{database_of, statuses_over},
    tenant_resource::TenantStatus,
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use std::time::Duration;

/// How long the tenant is watched after it was seen dormant: past the 30 s the pooler keeps an idle server connection.
const WATCH: Duration = Duration::from_secs(60);

/// How often its status is read.
const READ_EVERY: Duration = Duration::from_millis(250);

/// Pins: a tenant holding only an Entity, with no Subscription, no expiry and no delivery pending, once evicted under
/// the pressure of 40 other tenants and seen dormant, reads `dormant` at every read for 60 s and then holds no client
/// backend on its database
/// Clause: 5.5.10
/// Tenant: private (the subject is a whole tenant leaving the runtime cache)
/// Recorded: Andromeda extension, not v1.9.1: the tenant `status` (the /tenants resources) tells a loaded tenant from
/// an unloaded one, and reading it loads nothing
#[gtest]
#[tokio::test]
async fn p_dor_wak_0001_a_tenant_with_nothing_due_stays_dormant_and_holds_no_backend() -> Result<()> {
    let namespace = Namespace::fresh();
    let dormant = broker::client(BrokerRole::Dormant)?;
    let set = TenantSet::provision(&dormant, &namespace, 1, 1).await?;
    let member = set.member(0);
    let database = database_of(&dormant, &member.tenant).await?;
    evict_by_pressure(&dormant, &member.tenant).await?;
    let statuses = statuses_over(&dormant, &member.tenant, WATCH, READ_EVERY).await?;
    expect_that!(statuses, each(eq(&TenantStatus::Dormant)));
    let backends = ServerBackends::of(BrokerRole::Dormant)?.count(&BackendScope::Database(database)).await?;
    verify_that!(backends, eq(0))
}
