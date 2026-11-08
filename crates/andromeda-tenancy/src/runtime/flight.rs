//! Loading work run at most once per key at a time, detached from the
//! requests waiting for it.
//!
//! Creating and migrating a new tenant's database, and building a tenant's
//! runtime, can outlast the request deadline on a loaded server. Run inside a
//! request, the deadline drops the work part way through, the next request
//! starts again from the beginning, and under sustained load the tenant is
//! never loaded. Here each piece of loading work runs to its end in a
//! supervised slot, and a request waits for it only until its admission
//! deadline. The request is then shed as saturated, and the request that
//! retries joins the work still under way or finds it done.

use crate::{error::TenancyError, runtime::admission::saturated, saturated_wait::SaturatedWait, tenant_ref::TenantRef};
use andromeda_task_runtime::keyed_slots::{Admission, KeyedSlots};
use andromeda_telemetry::tenancy::record_build_admission_wait;
use dashmap::{DashMap, mapref::entry::Entry};
use std::{
    hash::Hash,
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

/// The id the next flight is offered under, unique across every set of
/// flights sharing one set of slots.
static NEXT_SLOT: AtomicU64 = AtomicU64::new(0);

/// The slot one flight runs under.
///
/// Each flight is offered under an id no other has held, so the slots never
/// refuse one as busy. Work for one key runs at most once at a time because
/// the flights join every request for the key to the work under way.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FlightSlot(u64);

impl FlightSlot {
    /// An id no flight has held yet.
    fn next() -> FlightSlot {
        FlightSlot(NEXT_SLOT.fetch_add(1, Ordering::Relaxed))
    }
}

/// How a flight ended, `None` while it is under way.
type Landed<T> = Option<Result<T, TenancyError>>;

/// The work under way for each key, and the slots it runs in.
pub(super) struct Flights<K: Eq + Hash, T> {
    /// Each key whose work is under way, with the channel its outcome is
    /// published on.
    in_flight: Arc<DashMap<K, watch::Receiver<Landed<T>>>>,
    /// The supervised slots every flight runs in.
    slots: KeyedSlots<FlightSlot>,
}

impl<K: Eq + Hash + Clone + Send + Sync + 'static, T: Clone + Send + Sync + 'static> Flights<K, T> {
    /// No work under way, each flight to run in `slots`.
    pub(super) fn new(slots: KeyedSlots<FlightSlot>) -> Flights<K, T> {
        Flights {
            in_flight: Arc::new(DashMap::new()),
            slots,
        }
    }

    /// Whether work for `key` is under way.
    pub(super) fn is_in_flight(&self, key: &K) -> bool {
        self.in_flight.contains_key(key)
    }

    /// The outcome of the work under way for `key`, waited for until
    /// `deadline`, or `None` when none is under way.
    ///
    /// The wait is shed as [`TenancyError::Saturated`] naming `wait` for
    /// `tenant`, as in [`Flights::outcome`].
    pub(super) async fn joined(&self, key: &K, tenant: &TenantRef, deadline: Instant, wait: SaturatedWait) -> Option<Result<T, TenancyError>> {
        let outcome = self.in_flight.get(key).map(|flight| flight.value().clone())?;
        Some(awaited(outcome, tenant, deadline, wait).await)
    }

    /// The outcome of the work for `key`, joining the work under way or
    /// launching `work` when none is, waited for until `deadline`.
    ///
    /// A launch first takes one of `permits` and hands it to the work, which
    /// holds it until it ends; a request waiting for the work holds none.
    ///
    /// # Errors
    ///
    /// Returns [`TenancyError::Saturated`] naming
    /// [`SaturatedWait::BuildPermit`] when no permit frees before `deadline`,
    /// in which case the work never starts, and naming `wait` when the work
    /// has not ended by `deadline` or was cut off when the broker shut down;
    /// the work then runs on, or resumes on the next launch. Otherwise returns
    /// whatever the work ended with, every request waiting for it reading the
    /// same outcome.
    pub(super) async fn outcome<W: Future<Output = Result<T, TenancyError>> + Send + 'static>(
        &self,
        key: &K,
        tenant: &TenantRef,
        deadline: Instant,
        wait: SaturatedWait,
        permits: &Arc<Semaphore>,
        work: impl FnOnce() -> W,
    ) -> Result<T, TenancyError> {
        let reserved = match self.in_flight.entry(key.clone()) {
            Entry::Occupied(flight) => Err(flight.get().clone()),
            Entry::Vacant(free) => {
                let (publish, outcome) = watch::channel(None);
                drop(free.insert(outcome.clone()));
                Ok((publish, outcome))
            }
        };
        let (publish, outcome) = match reserved {
            Ok(reserved) => reserved,
            Err(joined) => return awaited(joined, tenant, deadline, wait).await,
        };
        let landing = Landing {
            in_flight: Arc::clone(&self.in_flight),
            key: key.clone(),
            outcome: outcome.clone(),
        };
        let admission_started = StdInstant::now();
        let admitted = timeout_at(deadline, Arc::clone(permits).acquire_owned()).await;
        record_build_admission_wait(admission_started.elapsed().as_secs_f64());
        let Ok(Ok(permit)) = admitted else {
            // The reservation goes with its unpublished channel, which every
            // request that joined it meanwhile reads as cut off.
            drop(landing);
            return Err(saturated(tenant, SaturatedWait::BuildPermit));
        };
        let work = work();
        let flight = async move {
            let landed = work.await;
            drop(permit);
            publish.send_replace(Some(landed));
            drop(landing);
        };
        // A refused or cancelled flight drops its publisher unread, which every
        // request waiting for it reads as cut off.
        match self.slots.offer(FlightSlot::next(), flight, || {}) {
            Admission::Started | Admission::Busy => {}
        }
        awaited(outcome, tenant, deadline, wait).await
    }
}

/// What a flight ended with, waited for on `outcome` until `deadline`, or the
/// refusal naming `wait` when it has not ended by then or was cut off.
async fn awaited<T: Clone + Send + Sync>(
    mut outcome: watch::Receiver<Landed<T>>,
    tenant: &TenantRef,
    deadline: Instant,
    wait: SaturatedWait,
) -> Result<T, TenancyError> {
    let landed = match timeout_at(deadline, outcome.wait_for(Option::is_some)).await {
        Ok(Ok(published)) => (*published).clone(),
        Ok(Err(_)) | Err(_) => None,
    };
    landed.unwrap_or_else(|| Err(saturated(tenant, wait)))
}

/// Clears a key's flight however its work ends: run out, refused by the
/// slots, cut off at shutdown, or never admitted.
struct Landing<K: Eq + Hash, T> {
    /// The flights the work is recorded in.
    in_flight: Arc<DashMap<K, watch::Receiver<Landed<T>>>>,
    /// The key the work is for.
    key: K,
    /// This flight's outcome channel, which tells its entry apart.
    outcome: watch::Receiver<Landed<T>>,
}

impl<K: Eq + Hash, T> Drop for Landing<K, T> {
    // Clearing the flight is cleanup that must run however the work ends,
    // which no derive expresses.
    fn drop(&mut self) {
        self.in_flight.remove_if(&self.key, |_, held| held.same_channel(&self.outcome));
    }
}
