//! A new tenant is not starved by a full pooler.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, SuiteConfig},
    load_outcome::LoadOutcome,
    load_run::run_load,
    namespace::Namespace,
    tenant_picker::UniformPicker,
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::Entity;
use std::time::Duration;
use tokio::time::Instant;

/// The workers that fill the pooler.
const WORKERS: usize = 64;

/// How long they read.
const FILL: Duration = Duration::from_secs(10);

/// The first writes sent at once while the tenants are brought up.
const CONCURRENCY: usize = 32;

/// The longest the new tenant's first write may take.
const PROMPT: Duration = Duration::from_secs(5);

/// Pins: right after 64 workers read the Devices of the scale tests' tenant count, all kept resident, for 10 s, two
/// connections per tenant against the pooler's `max_user_connections` of 120 its idle servers then hold, a first
/// write into a tenant never written before is answered 201 within 5 s
/// Clause: 5.5.10, 5.6.1
/// Tenant: private (the subject is the implicit creation of a tenant among many)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`
/// whose servers stay open 30 s after their last transaction
#[gtest]
#[tokio::test]
async fn p_scl_res_0003_a_new_tenant_is_not_starved_by_a_full_pooler() -> Result<()> {
    let pooling = SuiteConfig::get()?.pooling()?;
    let crowded = broker::quiet_client(BrokerRole::Crowded)?;
    let set = TenantSet::provision(&crowded, &Namespace::fresh(), pooling.scale_tenants(), CONCURRENCY).await?;
    let picker = UniformPicker::new(set.len(), 3)?;
    let fill = run_load(WORKERS, FILL, |_| set.read(picker.pick())).await;
    println!("{fill}");
    let newcomer = Namespace::fresh();
    let device = Entity::builder()
        .id(newcomer.urn("Device", 1)?)
        .entity_type(newcomer.entity_type("Device")?)
        .build()?;
    let started = Instant::now();
    let created = LoadOutcome::of(&crowded.with_tenant(newcomer.tenant()?).with_responses().entities().create(&device).send().await);
    let took = started.elapsed();
    expect_that!(created.status(), some(eq(StatusCode::CREATED)));
    verify_that!(took, lt(PROMPT))
}
