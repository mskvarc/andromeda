//! Evicting one tenant from a broker's runtime cache by the pressure of others, and seeing it dormant.
//!
//! A test brings up more fresh tenants than the cache holds and reads each, so its own tenant, the least recently
//! used, is evicted; it watches the tenant's status meanwhile, which never loads it. Other tests' tenants on the same
//! broker only add to the pressure.

use crate::{
    broker::BrokerClient,
    namespace::Namespace,
    tenant_management::{TenantManagementError, await_status},
    tenant_resource::TenantStatus,
    tenant_set::{TenantSet, TenantSetError},
};
use ngsi_ld::model::Tenant;
use std::time::Duration;
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

/// Evicts `target` from the broker `broker` speaks to by bringing up and reading [`EVICTION_PRESSURE`] fresh tenants,
/// and answers when `target` was first seen `dormant`.
///
/// # Errors
///
/// Returns [`EvictionError`] when the other tenants cannot be brought up or `target` is not seen dormant within
/// [`DORMANCY_BOUND`].
pub async fn evict_by_pressure(broker: &BrokerClient, target: &Tenant) -> Result<Instant, EvictionError> {
    let management = broker.without_tenant();
    let watch = watch_dormant(&management, target);
    let push = async {
        let others = TenantSet::provision(&management, &Namespace::fresh(), EVICTION_PRESSURE, PRESSURE_CONCURRENCY).await?;
        for index in 0..others.len() {
            // Each read only moves its tenant to the front of the cache; how it is answered is not the subject.
            let _touched = others.read(index).await;
        }
        Ok::<(), TenantSetError>(())
    };
    let (seen, pushed) = tokio::join!(watch, push);
    pushed?;
    Ok(seen?)
}

/// Evicts `target` by reading every tenant of `others`, which the caller brought up beforehand, so the eviction
/// begins as soon as the reads do, and answers when `target` was first seen `dormant`.
///
/// `others` must hold more tenants than the broker's cache holds beside `target`.
///
/// # Errors
///
/// Returns [`EvictionError`] when `target` is not seen dormant within [`DORMANCY_BOUND`].
pub async fn evict_by_reading(broker: &BrokerClient, target: &Tenant, others: &TenantSet) -> Result<Instant, EvictionError> {
    let management = broker.without_tenant();
    let touch = async {
        for index in 0..others.len() {
            // Each read only moves its tenant to the front of the cache; how it is answered is not the subject.
            let _touched = others.read(index).await;
        }
    };
    let (seen, ()) = tokio::join!(watch_dormant(&management, target), touch);
    Ok(seen?)
}

/// Answers when `target` was first seen `dormant`, watching its status without loading it.
async fn watch_dormant(management: &BrokerClient, target: &Tenant) -> Result<Instant, TenantManagementError> {
    await_status(management, target, TenantStatus::Dormant, DORMANCY_BOUND)
        .await
        .map(|()| Instant::now())
}
