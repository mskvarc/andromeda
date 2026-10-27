//! Settling provisional tenants: establishing every one whose database holds
//! information, and retiring those a refused creating operation left behind.

use crate::{
    database::TenantDatabase,
    provisional::{Establishment, Retirement},
    runtime::TenantRuntimeManager,
    state_factory::TenantStateFactory,
    tenant_ref::TenantRef,
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use std::sync::Arc;
use tracing::warn;

/// What one sweep of the provisional tenants settled.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct ProvisionalSettlement {
    /// Tenants found to hold information and recorded as established.
    pub(super) established: usize,
    /// Tenants that stayed empty past their time to live, whose databases were
    /// dropped and rows removed.
    pub(super) retired: usize,
}

impl<D: TenantDatabase, F: TenantStateFactory> TenantRuntimeManager<D, F> {
    /// Settle every provisional tenant: establish one whose database holds
    /// information, and offer one this broker holds no runtime for to the
    /// registry for retirement.
    ///
    /// A tenant only ever written to is never read with a request that
    /// requires it to exist, so this sweep is what establishes it, and with it
    /// what admits it to the database maintenance that visits established
    /// tenants only.
    pub(super) async fn settle_provisional(&self) -> ProvisionalSettlement {
        let Ok(names) = self.database.provisional_tenants().await else {
            // An unreadable registry settles nothing; the next tick reads again.
            return ProvisionalSettlement::default();
        };
        let mut settlement = ProvisionalSettlement::default();
        for name in names {
            let tenant = TenantRef::Named(name.clone()); // clone: the cache and the lock map are keyed by an owned reference
            if self.runtimes.contains_key(&tenant) {
                self.establish_cached(&name, &mut settlement).await;
            } else {
                self.offer_for_retirement(&name, &tenant, &mut settlement).await;
            }
        }
        settlement
    }

    /// Establish a provisional tenant a cached runtime serves, if its database
    /// holds information.
    ///
    /// Never retires it: a request may be using the runtime.
    async fn establish_cached(&self, name: &TenantName, settlement: &mut ProvisionalSettlement) {
        match self.establish_tenant(name).await {
            Ok(Establishment::Established) => settlement.established += 1,
            Ok(Establishment::Empty) => {}
            Err(error) => warn!(tenant = name.as_str(), error = %error, "failed to establish a provisional tenant"),
        }
    }

    /// Offer a provisional tenant with no cached runtime to the registry for
    /// retirement, which establishes it instead when its database holds
    /// information.
    ///
    /// The tenant's build lock is held across the offer, so no request on this
    /// broker cold-builds its runtime while its database is dropped; such a
    /// request waits for the lock and then provisions the tenant afresh. A
    /// tenant whose build lock is taken is skipped rather than waited for, so
    /// the janitor never queues behind a cold build.
    async fn offer_for_retirement(&self, name: &TenantName, tenant: &TenantRef, settlement: &mut ProvisionalSettlement) {
        let build_lock = self.build_lock_for(tenant);
        let Ok(guard) = build_lock.try_lock() else {
            return;
        };
        let outcome = if self.runtimes.contains_key(tenant) {
            Ok(Retirement::Kept)
        } else {
            self.database.retire_provisional(name, self.config.provisional_tenant_lifetime.duration()).await
        };
        self.build_locks.remove_if(tenant, |_, existing| Arc::ptr_eq(existing, &build_lock));
        drop(guard);
        match outcome {
            Ok(Retirement::Retired) => settlement.retired += 1,
            Ok(Retirement::Established) => settlement.established += 1,
            Ok(Retirement::Kept) => {}
            Err(error) => warn!(tenant = name.as_str(), error = %error, "failed to retire a provisional tenant"),
        }
    }
}
