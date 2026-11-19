//! Evicting one tenant from a broker's runtime cache by the pressure of others, and seeing it dormant.
//!
//! A test brings up more fresh tenants than the cache holds and reads them, so its own tenant, the least recently
//! used, is evicted; it watches the tenant's status meanwhile, which never loads it. Other tests' tenants on the same
//! broker only add to the pressure. The broker unloads a tenant whose periodic notification falls due within its
//! minimum wake interval only after every tenant with nothing so close, so a tenant notifying every few seconds is a
//! candidate only between its notifications. The reads therefore go round the other tenants until the test's own is
//! seen dormant, rather than once, which a burst of loads can finish while the tenant is never a candidate.

use crate::{
    broker::BrokerClient,
    namespace::Namespace,
    tenant_management::{TenantManagementError, await_status},
    tenant_resource::TenantStatus,
    tenant_set::{TenantSet, TenantSetError},
};
use ngsi_ld::model::Tenant;
use std::{convert::Infallible, time::Duration};
use tokio::time::Instant;

/// How many other tenants a test touches: more than the cache of 32 the dormant broker keeps.
pub const EVICTION_PRESSURE: usize = 40;

/// How many first writes of the pressure run at once.
const PRESSURE_CONCURRENCY: usize = 16;

/// The longest a tenant is watched before it must have been seen dormant: the pressure itself, plus the minute a
/// tenant loaded for its due work in the meantime stays resident (`tenancy.residency.woken_lifetime`) before it can be
/// evicted again.
pub const DORMANCY_BOUND: Duration = Duration::from_secs(150);

/// Why a tenant was not seen evicted.
#[derive(Debug, thiserror::Error)]
pub enum EvictionError {
    /// The other tenants could not be brought up.
    #[error(transparent)]
    Pressure(#[from] TenantSetError),
    /// The tenant was not seen dormant.
    #[error(transparent)]
    Dormancy(#[from] TenantManagementError),
}

/// Evicts `target` from the broker `broker` speaks to by bringing up [`EVICTION_PRESSURE`] fresh tenants and reading
/// them until `target` is seen `dormant`, and answers when it was.
///
/// # Errors
///
/// Returns [`EvictionError`] when the other tenants cannot be brought up or `target` is not seen dormant within
/// [`DORMANCY_BOUND`].
pub async fn evict_by_pressure(broker: &BrokerClient, target: &Tenant) -> Result<Instant, EvictionError> {
    let management = broker.without_tenant();
    tokio::select! {
        seen = watch_dormant(&management, target) => Ok(seen?),
        refused = press(&management) => match refused {
            Err(error) => Err(error.into()),
        },
    }
}

/// Evicts `target` by reading the tenants of `others`, which the caller brought up beforehand, so the eviction
/// begins as soon as the reads do, until `target` is seen `dormant`, and answers when it was.
///
/// `others` must hold more tenants than the broker's cache holds beside `target`.
///
/// # Errors
///
/// Returns [`EvictionError`] when `target` is not seen dormant within [`DORMANCY_BOUND`].
pub async fn evict_by_reading(broker: &BrokerClient, target: &Tenant, others: &TenantSet) -> Result<Instant, EvictionError> {
    let management = broker.without_tenant();
    tokio::select! {
        seen = watch_dormant(&management, target) => Ok(seen?),
        never = keep_reading(others) => match never {},
    }
}

/// Brings up [`EVICTION_PRESSURE`] fresh tenants and reads them for as long as the caller lets it run.
async fn press(management: &BrokerClient) -> Result<Infallible, TenantSetError> {
    let others = TenantSet::provision(management, &Namespace::fresh(), EVICTION_PRESSURE, PRESSURE_CONCURRENCY).await?;
    match keep_reading(&others).await {}
}

/// Reads every tenant of `others` in turn, round after round, for as long as the caller lets it run.
async fn keep_reading(others: &TenantSet) -> Infallible {
    loop {
        for index in 0..others.len() {
            // Each read only moves its tenant to the front of the cache; how it is answered is not the subject.
            let _touched = others.read(index).await;
        }
    }
}

/// Answers when `target` was first seen `dormant`, watching its status without loading it.
async fn watch_dormant(management: &BrokerClient, target: &Tenant) -> Result<Instant, TenantManagementError> {
    await_status(management, target, TenantStatus::Dormant, DORMANCY_BOUND)
        .await
        .map(|()| Instant::now())
}
