//! Retiring provisional tenants that a refused creating operation left behind.

use crate::{database::TenantDatabase, provisional::Retirement, runtime::TenantRuntimeManager, state_factory::TenantStateFactory, tenant_ref::TenantRef};
use std::sync::Arc;
use tracing::warn;

impl<D: TenantDatabase, F: TenantStateFactory> TenantRuntimeManager<D, F> {
    /// Offer every provisional tenant this broker holds no runtime for to the
    /// registry for retirement, returning how many were retired.
    ///
    /// A tenant with a cached runtime is skipped: a request may be using it. The
    /// tenant's build lock is held across the offer, so no request on this
    /// broker cold-builds its runtime while its database is dropped; such a
    /// request waits for the lock and then provisions the tenant afresh. A
    /// tenant whose build lock is taken is skipped rather than waited for, so
    /// the janitor never queues behind a cold build.
    pub(super) async fn retire_abandoned_provisional(&self) -> usize {
        let Ok(names) = self.database.provisional_tenants().await else {
            // An unreadable registry retires nothing; the next tick reads again.
            return 0;
        };
        let idle_for = self.config.provisional_tenant_ttl.duration();
        let mut retired = 0;
        for name in names {
            let tenant = TenantRef::Named(name.clone()); // clone: the cache and the lock map are keyed by an owned reference
            if self.runtimes.contains_key(&tenant) {
                continue;
            }
            let build_lock = self.build_lock_for(&tenant);
            let Ok(guard) = build_lock.try_lock() else {
                continue;
            };
            let outcome = if self.runtimes.contains_key(&tenant) {
                Ok(Retirement::Kept)
            } else {
                self.database.retire_provisional(&name, idle_for).await
            };
            self.build_locks.remove_if(&tenant, |_, existing| Arc::ptr_eq(existing, &build_lock));
            drop(guard);
            match outcome {
                Ok(Retirement::Retired) => retired += 1,
                Ok(Retirement::Established | Retirement::Kept) => {}
                Err(error) => warn!(tenant = name.as_str(), error = %error, "failed to retire a provisional tenant"),
            }
        }
        retired
    }
}
