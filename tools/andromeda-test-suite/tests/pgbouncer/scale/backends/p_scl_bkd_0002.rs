//! Tripled client concurrency opens no backend beyond the tenants' pools.

use andromeda_test_suite::{
    backend_sampler::BackendSampler,
    broker,
    config::{BrokerRole, SuiteConfig},
    load_outcome::LoadOutcome,
    load_run::run_load,
    namespace::Namespace,
    server_backends::{BackendScope, ServerBackends},
    tenant_picker::UniformPicker,
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::Entity;

/// The tenants the load spreads over: inside the 32 the broker keeps resident, so no request waits for a load.
const TENANTS: usize = 24;

/// The connections each tenant's pool opens through the pooler (`database.pooler.max_connections_per_tenant`).
const CONNECTIONS_PER_TENANT: u64 = 2;

/// The workers of the first run.
const WORKERS: usize = 16;

/// The workers of the second run: three times the first.
const TRIPLED_WORKERS: usize = 48;

/// Pins: 16 workers, then 48, retrieving the Devices of 24 resident tenants for the scale tests' load duration each
/// through a transaction pooler have every request answered with a success, and in the second run the client backends
/// on tenant databases never pass the tenants' pools, two connections each; an Entity then created in one of them
/// (201) is retrieved with a success
/// Clause: 5.7.1, 5.6.1
/// Tenant: private (the subject is the backends of whole tenant databases)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`,
/// two connections per tenant
#[gtest]
#[tokio::test]
async fn p_scl_bkd_0002_tripled_client_concurrency_opens_no_backend_beyond_the_tenants_pools() -> Result<()> {
    let pooling = SuiteConfig::get()?.pooling()?;
    let concurrency = broker::quiet_client(BrokerRole::Concurrency)?;
    let namespace = Namespace::fresh();
    let set = TenantSet::provision(&concurrency, &namespace, TENANTS, TENANTS).await?;
    let picker = UniformPicker::new(set.len(), 1)?;
    let first = run_load(WORKERS, pooling.load_duration(), |_| set.read(picker.pick())).await;
    let sampler = BackendSampler::start(ServerBackends::of(BrokerRole::Concurrency)?, BackendScope::TenantDatabases);
    let tripled = run_load(TRIPLED_WORKERS, pooling.load_duration(), |_| set.read(picker.pick())).await;
    let series = sampler.stop().await?;
    println!("first {first}; tripled {tripled}; tenant backends {series:?}");
    expect_that!(first.unsuccessful(), eq(0));
    expect_that!(tripled.unsuccessful(), eq(0));
    expect_that!(series.peak, le(u64::try_from(TENANTS)? * CONNECTIONS_PER_TENANT));
    let written = Entity::builder()
        .id(namespace.urn("Device", "written")?)
        .entity_type(namespace.entity_type("Device")?)
        .build()?;
    let member = set.member(0).client.with_responses();
    let created = LoadOutcome::of(&member.entities().create(&written).send().await);
    expect_that!(created.status(), some(eq(StatusCode::CREATED)));
    let read = LoadOutcome::of(&member.entities().retrieve(&written.id).send().await);
    verify_that!(read.is_success(), eq(true))
}
