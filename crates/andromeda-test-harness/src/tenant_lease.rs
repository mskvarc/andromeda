//! Exclusive tenants leased to concurrently running multi-broker tests.
//!
//! An identifier prefix does not isolate two federation tests that share a
//! tenant. Clause 5.12 matches a type-only or entity-less registration against
//! every request of its tenant whatever the identifiers asked for, and a query
//! without an identifier filter reads every entity of its type, so one test's
//! retrieval reaches another test's context sources. Each topology therefore
//! runs under a tenant no concurrently running test uses (clause 4.14: every
//! tenant is its own set of entities, registrations and subscriptions, and
//! clause 4.3.6.4 federates each tenant separately).
//!
//! A tenant is a database per broker, so the tenants are reused rather than
//! minted per test: a finished topology hands its tenant back, cleaned, to the
//! next one. A topology that never finishes -- a test that panicked -- leaves
//! its tenant out of the pool, so no later test inherits what it left behind.

use crate::identifiers::TestSuiteName;
use andromeda_test_client::{tenant_name::TenantName, transport_error::TenantNameError};
use std::sync::{
    LazyLock,
    Mutex,
    atomic::{AtomicU32, Ordering},
};

/// The released slots a later topology may lease again.
static IDLE_SLOTS: LazyLock<Mutex<Vec<LeaseSlot>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// The next slot index minted when no released slot fits.
static NEXT_SLOT: AtomicU32 = AtomicU32::new(0);

/// One reusable tenant of one suite.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LeaseSlot {
    /// The suite the tenant belongs to, which names it.
    suite: TestSuiteName,
    /// The slot's index, unique within the process.
    index: u32,
}

impl LeaseSlot {
    /// The tenant name the slot runs under: the suite token followed by the
    /// slot index, both of which are valid in a header value and in a database
    /// name.
    #[must_use]
    pub fn label(&self) -> String {
        format!("{}lease{}", self.suite.as_str(), self.index)
    }
}

/// A tenant held exclusively by one topology until it is released.
#[derive(Debug)]
pub struct TenantLease {
    slot: LeaseSlot,
    tenant: TenantName,
}

impl TenantLease {
    /// Lease a released tenant of `suite`, or a new one when every tenant of the
    /// suite is held.
    ///
    /// # Errors
    ///
    /// Returns [`TenantNameError`] when the tenant name the slot derives is not
    /// a valid header value, which a validated suite token never produces.
    pub fn acquire(suite: &TestSuiteName) -> Result<TenantLease, TenantNameError> {
        let released = {
            let mut idle = match IDLE_SLOTS.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            idle.iter().position(|slot| &slot.suite == suite).map(|position| idle.swap_remove(position))
        };
        let slot = released.unwrap_or_else(|| LeaseSlot {
            suite: suite.clone(), // clone: the slot outlives the suite name it was minted for
            index: NEXT_SLOT.fetch_add(1, Ordering::Relaxed),
        });
        let tenant = TenantName::parse(slot.label())?;
        Ok(TenantLease { slot, tenant })
    }

    /// Borrow the slot the lease holds.
    #[must_use]
    pub const fn slot(&self) -> &LeaseSlot {
        &self.slot
    }

    /// Borrow the tenant every client of the topology runs under.
    #[must_use]
    pub const fn tenant(&self) -> &TenantName {
        &self.tenant
    }

    /// Hand the tenant back for a later topology to lease.
    ///
    /// Only a topology that removed everything it created releases its lease;
    /// one that is dropped instead abandons the tenant.
    pub fn release(self) {
        let mut idle = match IDLE_SLOTS.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        idle.push(self.slot);
    }
}

#[cfg(test)]
mod tests {
    use crate::{identifiers::TestSuiteName, tenant_lease::TenantLease};

    fn suite(token: &str) -> TestSuiteName {
        TestSuiteName::parse(token).expect("valid suite token")
    }

    #[test]
    fn concurrently_held_leases_never_share_a_tenant() {
        let suite = suite("leaseconcurrent");
        let first = TenantLease::acquire(&suite).expect("first lease");
        let second = TenantLease::acquire(&suite).expect("second lease");
        assert_ne!(first.slot(), second.slot());
        assert_ne!(first.tenant(), second.tenant());
    }

    #[test]
    fn a_released_tenant_is_leased_again() {
        let suite = suite("leasereuse");
        let first = TenantLease::acquire(&suite).expect("first lease");
        let released = first.slot().clone();
        first.release();
        let second = TenantLease::acquire(&suite).expect("second lease");
        pretty_assertions::assert_eq!(second.slot(), &released);
    }

    #[test]
    fn an_abandoned_tenant_is_never_leased_again() {
        let suite = suite("leaseabandon");
        let first = TenantLease::acquire(&suite).expect("first lease");
        let abandoned = first.slot().clone();
        drop(first);
        let second = TenantLease::acquire(&suite).expect("second lease");
        assert_ne!(second.slot(), &abandoned);
    }

    #[test]
    fn a_released_tenant_of_another_suite_is_not_leased() {
        let other = TenantLease::acquire(&suite("leaseother")).expect("other suite lease");
        let released = other.slot().clone();
        other.release();
        let lease = TenantLease::acquire(&suite("leasemine")).expect("own lease");
        assert_ne!(lease.slot(), &released);
        assert!(lease.slot().label().starts_with("leasemine"));
    }
}
