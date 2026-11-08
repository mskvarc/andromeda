//! Provisioning each tenant at most once at a time, detached from the requests
//! waiting for it.
//!
//! Creating and migrating a tenant database can outlast the request deadline
//! on a loaded server. Run inside a request, the deadline drops it part way
//! through a migration, the next request starts again from the beginning, and
//! under sustained load the tenant never comes into existence. Here each
//! provisioning runs to its end in a supervised slot, and a request waits for
//! it only until its admission deadline. The request is then shed as
//! saturated, and the request that retries joins the provisioning still under
//! way or finds the tenant prepared.

use crate::{database::TenantDatabase, error::TenancyError, runtime::admission::saturated, saturated_wait::SaturatedWait, tenant_ref::TenantRef};
use andromeda_ngsi_ld_lexicon::{tenant::TenantName, tenant_database_name::TenantDatabaseName};
use andromeda_task_runtime::keyed_slots::{Admission, KeyedSlots};
use andromeda_telemetry::tenancy::record_build_admission_wait;
use dashmap::{DashMap, mapref::entry::Entry};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant as StdInstant,
};
use tokio::{
    sync::{Semaphore, watch},
    time::{Instant, timeout_at},
};

/// The slot one provisioning runs under.
///
/// Each provisioning is offered under an id no other has held, so the slots
/// never refuse one as busy. A tenant is provisioned at most once at a time
/// because the flights join every request for it to the one
/// provisioning under way.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ProvisioningSlot(u64);

/// How a provisioning ended, `None` while it is under way.
type Landed = Option<Result<TenantDatabaseName, TenancyError>>;

/// The provisioning under way for each tenant, and the slots it runs in.
pub(super) struct ProvisioningFlights {
    /// Each tenant whose provisioning is under way, with the channel its
    /// outcome is published on.
    in_flight: Arc<DashMap<TenantName, watch::Receiver<Landed>>>,
    /// The supervised slots every provisioning runs in.
    slots: KeyedSlots<ProvisioningSlot>,
    /// The id the next provisioning is offered under.
    next_slot: AtomicU64,
}

impl ProvisioningFlights {
    /// No provisioning under way, each one to run in `slots`.
    pub(super) fn new(slots: KeyedSlots<ProvisioningSlot>) -> ProvisioningFlights {
        ProvisioningFlights {
            in_flight: Arc::new(DashMap::new()),
            slots,
            next_slot: AtomicU64::new(0),
        }
    }

    /// Whether the provisioning of `name` is under way, its database possibly
    /// still migrating.
    pub(super) fn is_in_flight(&self, name: &TenantName) -> bool {
        self.in_flight.contains_key(name)
    }

    /// The database of `name` once it is provisioned, joining the provisioning
    /// under way or launching one, waited for until `deadline`.
    ///
    /// A provisioning launched here takes one of `permits`, the budget it
    /// shares with runtime builds, before it starts, and runs to its end
    /// whether or not anyone still waits for it.
    ///
    /// # Errors
    ///
    /// Returns [`TenancyError::Saturated`] naming
    /// [`SaturatedWait::Provisioning`] when the provisioning has not ended by
    /// `deadline` or was cut off when the broker shut down, and whatever the
    /// provisioning failed with otherwise, every request waiting for it reading
    /// the same failure.
    pub(super) async fn provisioned<D: TenantDatabase>(
        &self,
        database: &Arc<D>,
        permits: &Arc<Semaphore>,
        name: &TenantName,
        deadline: Instant,
    ) -> Result<TenantDatabaseName, TenancyError> {
        let mut outcome = self.join_or_launch(database, permits, name, deadline);
        let landed = match timeout_at(deadline, outcome.wait_for(Option::is_some)).await {
            Ok(Ok(published)) => (*published).clone(),
            Ok(Err(_)) | Err(_) => None,
        };
        landed.unwrap_or_else(|| Err(saturated(&TenantRef::Named(name.clone()), SaturatedWait::Provisioning)))
    }

    /// The outcome channel of the provisioning under way for `name`, launching
    /// one when none is.
    fn join_or_launch<D: TenantDatabase>(&self, database: &Arc<D>, permits: &Arc<Semaphore>, name: &TenantName, deadline: Instant) -> watch::Receiver<Landed> {
        let (publish, outcome) = match self.in_flight.entry(name.clone()) {
            Entry::Occupied(flight) => return flight.get().clone(),
            Entry::Vacant(free) => {
                let (publish, outcome) = watch::channel(None);
                drop(free.insert(outcome.clone()));
                (publish, outcome)
            }
        };
        let landing = Landing {
            in_flight: Arc::clone(&self.in_flight),
            name: name.clone(),
            outcome: outcome.clone(),
        };
        let database = Arc::clone(database);
        let permits = Arc::clone(permits);
        let slot = ProvisioningSlot(self.next_slot.fetch_add(1, Ordering::Relaxed));
        let work = async move {
            let provisioned = provision_when_admitted(database.as_ref(), &permits, &landing.name, deadline).await;
            publish.send_replace(Some(provisioned));
            drop(landing);
        };
        // A refused or cancelled provisioning drops its publisher unread, which
        // every request waiting for it reads as a provisioning cut off.
        match self.slots.offer(slot, work, || {}) {
            Admission::Started | Admission::Busy => {}
        }
        outcome
    }
}

/// Provision `name` under one of `permits`, unless none frees before
/// `deadline`.
///
/// A provisioning not under way by the deadline of the request that launched
/// it is dropped as that request is, so a burst of names nobody waits for any
/// more leaves no provisioning queued behind it.
async fn provision_when_admitted<D: TenantDatabase>(
    database: &D,
    permits: &Semaphore,
    name: &TenantName,
    deadline: Instant,
) -> Result<TenantDatabaseName, TenancyError> {
    let refused = || saturated(&TenantRef::Named(name.clone()), SaturatedWait::BuildPermit);
    if Instant::now() >= deadline {
        return Err(refused());
    }
    let admission_started = StdInstant::now();
    let admitted = timeout_at(deadline, permits.acquire()).await;
    record_build_admission_wait(admission_started.elapsed().as_secs_f64());
    let Ok(Ok(_permit)) = admitted else {
        return Err(refused());
    };
    database.provision(name).await
}

/// Clears a tenant's provisioning from the flights however its work ends: run
/// out, refused by the slots, or cut off at shutdown.
struct Landing {
    /// The flights the provisioning is recorded in.
    in_flight: Arc<DashMap<TenantName, watch::Receiver<Landed>>>,
    /// The tenant being provisioned.
    name: TenantName,
    /// This provisioning's outcome channel, which tells its entry apart.
    outcome: watch::Receiver<Landed>,
}

impl Drop for Landing {
    // Clearing the flight is cleanup that must run however the work ends,
    // which no derive expresses.
    fn drop(&mut self) {
        self.in_flight.remove_if(&self.name, |_, held| held.same_channel(&self.outcome));
    }
}
