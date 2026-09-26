//! Exclusive tenants leased to concurrently running multi-broker tests.
//!
//! An identifier prefix does not isolate two federation tests that share a
//! tenant. Clause 5.12 matches a type-only or entity-less registration against
//! every request of its tenant whatever the identifiers asked for, and a query
//! without an identifier filter reads every entity of its type, so one test's
//! retrieval reaches another test's context sources. Each topology therefore
//! runs under a tenant no concurrently running test uses (clause 4.14: every
//! tenant is its own set of entities, registrations and subscriptions).
//!
//! A tenant is a database per broker, so the tenants form a fixed pool of
//! [`LEASED_TENANTS`] slots that is never extended: a topology waits for a slot
//! when every slot is held, and every slot comes back when its lease ends,
//! whatever became of the test. A slot whose last holder did not finish cleanly
//! -- its cleanup failed, or its test panicked -- comes back owing a reset of
//! every tenant database that holder touched, which the next holder performs
//! before it uses the slot.

use crate::identifiers::TestSuiteName;
use andromeda_test_client::{tenant_name::TenantName, transport_error::TenantNameError};
use std::{
    collections::HashSet,
    sync::{LazyLock, Mutex},
};
use tokio::sync::{Semaphore, SemaphorePermit};
use url::Url;

/// How many tenants the pool holds, and so how many topologies run at once.
///
/// It matches the `--test-threads` the multi-broker suites run at, so no test
/// waits in the pool at that degree; a run at a higher degree queues topologies
/// rather than creating more tenants, which keeps each broker's database
/// connections bounded however many tests run or fail.
pub const LEASED_TENANTS: usize = 4;

/// One permit per slot; a lease holds its permit for as long as it holds its slot.
static PERMITS: Semaphore = Semaphore::const_new(LEASED_TENANTS);

/// The slots no lease holds. A lease takes one only while holding a permit, and
/// there are as many permits as slots, so a permit always finds a slot.
static IDLE_SLOTS: LazyLock<Mutex<Vec<LeaseSlot>>> = LazyLock::new(|| {
    Mutex::new(
        (0..LEASED_TENANTS)
            .map(|index| LeaseSlot {
                index,
                owed_resets: HashSet::new(),
            })
            .collect(),
    )
});

/// One reusable tenant.
#[derive(Debug)]
struct LeaseSlot {
    /// The slot's index, which names its tenant.
    index: usize,
    /// The tenant databases a previous holder left without cleaning them.
    owed_resets: HashSet<Url>,
}

/// Return a slot to the pool.
fn park(slot: LeaseSlot) {
    let mut idle = match IDLE_SLOTS.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    idle.push(slot);
}

/// Whether a lease ended with its tenant clean.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LeaseEnd {
    /// The holder removed everything it created.
    Clean,
    /// The holder may have left data behind in every database it touched.
    Tainted,
}

/// A tenant held exclusively by one topology until the lease is dropped.
///
/// Dropping a lease without [`TenantLease::release_clean`] -- a failed cleanup
/// or a panicking test -- returns the slot owing a reset of every tenant
/// database recorded with [`TenantLease::record_database`].
#[derive(Debug)]
pub struct TenantLease {
    /// The slot, taken back when the lease drops.
    slot: Option<LeaseSlot>,
    label: String,
    tenant: TenantName,
    /// The tenant databases this lease has used.
    used: HashSet<Url>,
    end: LeaseEnd,
    /// Dropped after the slot is parked, so a waiting topology finds it.
    _permit: SemaphorePermit<'static>,
}

impl TenantLease {
    /// Wait for a free slot of the pool and lease its tenant, named after
    /// `suite`.
    ///
    /// # Errors
    ///
    /// Returns [`TenantLeaseError::Closed`] when the pool has been closed, which
    /// it never is, and [`TenantLeaseError::Tenant`] when the tenant name the
    /// slot derives is not a valid header value, which a validated suite token
    /// never produces.
    pub async fn acquire(suite: &TestSuiteName) -> Result<TenantLease, TenantLeaseError> {
        let permit = PERMITS.acquire().await.map_err(|_closed| TenantLeaseError::Closed)?;
        let slot = {
            let mut idle = match IDLE_SLOTS.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            idle.pop().ok_or(TenantLeaseError::Closed)?
        };
        let label = format!("{}lease{}", suite.as_str(), slot.index);
        let tenant = match TenantName::parse(&label) {
            Ok(tenant) => tenant,
            Err(error) => {
                park(slot);
                return Err(TenantLeaseError::Tenant(error));
            }
        };
        Ok(TenantLease {
            slot: Some(slot),
            label,
            tenant,
            used: HashSet::new(),
            end: LeaseEnd::Tainted,
            _permit: permit,
        })
    }

    /// Borrow the tenant every client of the topology runs under.
    #[must_use]
    pub const fn tenant(&self) -> &TenantName {
        &self.tenant
    }

    /// Borrow the tenant's name as the platform registry records it.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Record that the topology uses the tenant database at `database`, and
    /// answer whether a previous holder left it owing a reset, which the caller
    /// then performs before using it.
    pub fn record_database(&mut self, database: &Url) -> OwedReset {
        self.used.insert(database.clone()); // clone: the lease owns the set of databases it touched
        match self.slot.as_mut().map(|slot| slot.owed_resets.remove(database)) {
            Some(true) => OwedReset::Owed,
            Some(false) | None => OwedReset::Settled,
        }
    }

    /// End the lease with its tenant clean: the topology removed everything it
    /// created, so the next holder resets nothing on its account.
    pub fn release_clean(mut self) {
        self.end = LeaseEnd::Clean;
    }
}

impl Drop for TenantLease {
    fn drop(&mut self) {
        if let Some(mut slot) = self.slot.take() {
            match self.end {
                LeaseEnd::Clean => {}
                LeaseEnd::Tainted => slot.owed_resets.extend(self.used.drain()),
            }
            park(slot);
        }
    }
}

/// Whether a tenant database must be reset before its new holder uses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwedReset {
    /// A previous holder left data in it.
    Owed,
    /// It holds nothing a previous holder left.
    Settled,
}

/// Failure while leasing a tenant.
#[derive(Debug, thiserror::Error)]
pub enum TenantLeaseError {
    /// The pool no longer hands out tenants.
    #[error("the leased-tenant pool is closed")]
    Closed,
    /// The leased tenant is not a valid header value.
    #[error(transparent)]
    Tenant(#[from] TenantNameError),
}

#[cfg(test)]
mod tests {
    use crate::{
        identifiers::TestSuiteName,
        tenant_lease::{LEASED_TENANTS, OwedReset, TenantLease},
    };
    use std::time::Duration;
    use tokio::time::timeout;
    use url::Url;

    fn suite() -> TestSuiteName {
        TestSuiteName::parse("leasetest").expect("valid suite token")
    }

    fn database(name: &str) -> Url {
        Url::parse(&format!("postgres://127.0.0.1/{name}")).expect("valid URL")
    }

    /// The pool is process-wide, so its behaviour is asserted in one test that
    /// holds every slot at once rather than in tests racing for slots.
    #[tokio::test]
    async fn the_pool_is_bounded_reuses_every_slot_and_owes_resets_for_tainted_ones() {
        let mut held = Vec::new();
        for _ in 0..LEASED_TENANTS {
            held.push(TenantLease::acquire(&suite()).await.expect("a free slot"));
        }
        let mut tenants = held.iter().map(|lease| lease.tenant().clone()).collect::<Vec<_>>(); // clone: the assertion outlives the leases
        tenants.sort_by(|left, right| left.header_value().as_bytes().cmp(right.header_value().as_bytes()));
        tenants.dedup();
        pretty_assertions::assert_eq!(tenants.len(), LEASED_TENANTS, "concurrently held leases never share a tenant");

        // Every slot is held, so another topology waits rather than minting a tenant.
        assert!(
            timeout(Duration::from_millis(50), TenantLease::acquire(&suite())).await.is_err(),
            "a full pool makes the next lease wait"
        );

        // A lease dropped without a clean release -- a panicking test -- returns
        // its slot owing a reset of what it touched.
        let mut tainted = held.pop().expect("a held lease");
        let tainted_tenant = tainted.tenant().clone(); // clone: the lease is dropped below
        pretty_assertions::assert_eq!(tainted.record_database(&database("touched")), OwedReset::Settled);
        drop(tainted);
        let mut reused = TenantLease::acquire(&suite()).await.expect("the returned slot");
        pretty_assertions::assert_eq!(reused.tenant(), &tainted_tenant, "a failed lease's tenant is reused, not discarded");
        pretty_assertions::assert_eq!(reused.record_database(&database("touched")), OwedReset::Owed);
        pretty_assertions::assert_eq!(reused.record_database(&database("untouched")), OwedReset::Settled);

        // A clean release owes nothing, including the reset this holder performed.
        reused.release_clean();
        let mut clean = TenantLease::acquire(&suite()).await.expect("the returned slot");
        pretty_assertions::assert_eq!(clean.record_database(&database("touched")), OwedReset::Settled);
        clean.release_clean();
        for lease in held {
            lease.release_clean();
        }
    }
}
