//! An Entity expires in a tenant that was evicted before its expiry.

use andromeda_test_suite::{
    broker,
    clock_wait::wait_past,
    config::BrokerRole,
    eventually::retried,
    eviction::evict_by_pressure,
    namespace::Namespace,
    problem::answered,
    tenant_management::{TenantView, read_tenant},
    tenant_set::TenantSet,
};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{DateTime, Entity};

/// How far ahead the Entity expires: past the time the eviction takes, however many other tests evict on the broker.
const EXPIRES_IN: TimeDelta = TimeDelta::seconds(120);

/// Pins: an Entity created (201) with an `expiresAt` 120 s ahead, in a tenant then evicted under the pressure of 40
/// other tenants and seen dormant before that instant, is gone from the tenant's database after it: the tenant's
/// usage counts only the Entity its first write created
/// Clause: 4.22
/// Tenant: private (the subject is a whole tenant leaving the runtime cache)
/// Recorded: Andromeda extension, not v1.9.1: the tenant usage (`options=usage` of the /tenants resources) counts the
/// Entities the tenant's database holds without loading the tenant
#[gtest]
#[tokio::test]
async fn p_dor_exp_0001_an_entity_expires_in_a_tenant_that_was_evicted_before_its_expiry() -> Result<()> {
    let namespace = Namespace::fresh();
    let dormant = broker::client(BrokerRole::Dormant)?;
    let set = TenantSet::provision(&dormant, &namespace, 1, 1).await?;
    let member = set.member(0);
    let expires_at = Utc::now() + EXPIRES_IN;
    let expiring = Entity::builder()
        .id(namespace.urn("Device", "expiring")?)
        .entity_type(namespace.entity_type("Device")?)
        .expires_at(expires_at.format("%Y-%m-%dT%H:%M:%SZ").to_string().parse::<DateTime>()?)
        .build()?;
    let created = member.client.with_responses().entities().create(&expiring).send().await;
    expect_that!(&created, answered(StatusCode::CREATED));
    evict_by_pressure(&dormant, &member.tenant).await?;
    let dormant_before_expiry = Utc::now() < expires_at;
    expect_that!(dormant_before_expiry, eq(true));
    wait_past(expires_at.timestamp()).await;
    retried(async || {
        let usage = read_tenant(&dormant, &member.tenant, TenantView::Usage).await?;
        verify_that!(usage.entity_count, some(eq(1)))
    })
    .await
}
