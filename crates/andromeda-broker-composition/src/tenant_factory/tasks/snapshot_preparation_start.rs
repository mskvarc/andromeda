//! Starting a created or cloned Snapshot's preparation on the tenant's keyed
//! slots, ahead of the scheduled preparation sweep.
//!
//! A Snapshot holds one slot while its pass is queued or running, so a start
//! for a Snapshot already being prepared is dropped: the running pass covers
//! it, and the scheduled sweep stays the guarantee that every `preparing`
//! Snapshot is executed. The slots stop with the tenant, so no pass outlives
//! the runtime whose pool it reads.

use crate::tenant_factory::{error::TenantAssemblyError, tasks::tenant_slots::register_tenant_slots};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_snapshot::{
    error::SnapshotError,
    executor::{SnapshotPreparation, SnapshotPreparationOutcome},
    preparation_start::PreparationStart,
    status_delivery_signal::StatusDeliverySignal,
};
use andromeda_task_runtime::{keyed_slots::KeyedSlots, supervisor::TaskSupervisor};
use std::{num::NonZeroUsize, sync::Arc};
use tracing::{info, warn};

/// Create the tenant's create-time Snapshot preparation slots on its
/// `supervisor`, `width` wide: as many as one round of the scheduled
/// preparation drain claims and runs concurrently.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the supervisor refuses the
/// registration.
pub(crate) fn register_snapshot_preparation_slots(
    supervisor: &TaskSupervisor,
    width: NonZeroUsize,
) -> Result<KeyedSlots<Arc<Identifier>>, TenantAssemblyError> {
    register_tenant_slots(supervisor, "snapshot-create-preparations", width)
}

/// The tenant's create-time preparation start: `preparation`'s pass for one
/// created Snapshot, run on `slots`, waking status delivery through
/// `delivery_signal` once it prepared.
pub(crate) fn slotted_preparation_start(
    slots: KeyedSlots<Arc<Identifier>>,
    preparation: Arc<SnapshotPreparation>,
    delivery_signal: Arc<dyn StatusDeliverySignal>,
) -> Arc<dyn PreparationStart> {
    Arc::new(SlottedPreparationStart {
        slots,
        pass: move |snapshot: Arc<Identifier>| {
            let preparation = Arc::clone(&preparation);
            async move { preparation.execute_created(&snapshot).await }
        },
        delivery_signal,
    })
}

/// Runs each created Snapshot's preparation pass in a slot of its own.
struct SlottedPreparationStart<P> {
    /// The tenant's create-time preparation slots, keyed by Snapshot.
    slots: KeyedSlots<Arc<Identifier>>,
    /// Builds the create-time preparation pass of one Snapshot.
    pass: P,
    /// Wakes status delivery once a pass prepared a Snapshot.
    delivery_signal: Arc<dyn StatusDeliverySignal>,
}

impl<P, F> PreparationStart for SlottedPreparationStart<P>
where
    P: Fn(Arc<Identifier>) -> F + Send + Sync,
    F: Future<Output = Result<SnapshotPreparationOutcome, SnapshotError>> + Send + 'static,
{
    fn start(&self, snapshot: &Identifier) {
        let snapshot = Arc::new(snapshot.clone()); // clone: the pass outlives the request that created the Snapshot.
        let pass = (self.pass)(Arc::clone(&snapshot));
        let delivery_signal = Arc::clone(&self.delivery_signal);
        let key = Arc::clone(&snapshot);
        let work = async move {
            match pass.await {
                Ok(SnapshotPreparationOutcome::Idle) => {}
                Ok(SnapshotPreparationOutcome::Prepared { count }) => {
                    info!(count, "snapshot preparation completed on create");
                    // The preparation committed its status rows before returning,
                    // so wake delivery now instead of leaving them to the drain's
                    // idle fallback (the scheduled sweep will not, having found no
                    // preparing Snapshot left to complete).
                    delivery_signal.signal();
                }
                Err(error) => warn!(snapshot_id = %snapshot, %error, "snapshot preparation on create failed; the scheduled sweep will retry"),
            }
        };
        // A Snapshot whose pass is still queued or running is refused, and the
        // refusal is dropped: that pass covers it.
        let _admission = self.slots.offer(key, work, || {});
    }
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::tasks::snapshot_preparation_start::{SlottedPreparationStart, register_snapshot_preparation_slots};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_snapshot::{
        error::SnapshotError,
        executor::SnapshotPreparationOutcome,
        preparation_start::PreparationStart,
        status_delivery_signal::StatusDeliverySignal,
    };
    use andromeda_task_runtime::{keyed_slots::KeyedSlots, supervisor::TaskSupervisor};
    use std::{
        num::NonZeroUsize,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };
    use tokio::{runtime::Handle, sync::mpsc, task::yield_now, time::timeout};
    use tokio_util::sync::CancellationToken;

    /// Counts the status-delivery wakes raised.
    #[derive(Default)]
    struct CountedWakes(AtomicUsize);

    impl CountedWakes {
        fn count(&self) -> usize {
            self.0.load(Ordering::Acquire)
        }
    }

    impl StatusDeliverySignal for CountedWakes {
        fn signal(&self) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn snapshot(name: &str) -> Identifier {
        format!("urn:ngsi-ld:Snapshot:{name}").parse().expect("a valid snapshot identifier")
    }

    fn slots(width: usize) -> KeyedSlots<Arc<Identifier>> {
        KeyedSlots::new(NonZeroUsize::new(width).expect("a positive width"), CancellationToken::new())
    }

    /// What a fake pass ends with: `prepared` prepared one Snapshot,
    /// `failed` could not be claimed, every other Snapshot found nothing.
    fn outcome_of(ran_for: &Identifier) -> Result<SnapshotPreparationOutcome, SnapshotError> {
        if *ran_for == snapshot("prepared") {
            Ok(SnapshotPreparationOutcome::Prepared { count: 1 })
        } else if *ran_for == snapshot("failed") {
            Err(SnapshotError::StoreUnavailable)
        } else {
            Ok(SnapshotPreparationOutcome::Idle)
        }
    }

    /// A start on `slots` whose passes report the Snapshot they run for, wait
    /// until `gate` opens, and end as [`outcome_of`] says.
    fn gated_start(
        slots: KeyedSlots<Arc<Identifier>>,
        gate: CancellationToken,
        wakes: Arc<CountedWakes>,
    ) -> (impl PreparationStart, mpsc::UnboundedReceiver<Identifier>) {
        let (log, ran) = mpsc::unbounded_channel();
        let start = SlottedPreparationStart {
            slots,
            pass: move |snapshot: Arc<Identifier>| {
                let log = log.clone();
                let gate = gate.clone();
                async move {
                    log.send((*snapshot).clone()).expect("the test reads the log");
                    gate.cancelled().await;
                    outcome_of(&snapshot)
                }
            },
            delivery_signal: wakes,
        };
        (start, ran)
    }

    async fn next_ran(ran: &mut mpsc::UnboundedReceiver<Identifier>) -> Identifier {
        timeout(Duration::from_secs(5), ran.recv())
            .await
            .expect("a pass starts within the timeout")
            .expect("the passes still log")
    }

    async fn until_free(slots: &KeyedSlots<Arc<Identifier>>, snapshots: &[Identifier]) {
        timeout(Duration::from_secs(5), async {
            while snapshots.iter().any(|snapshot| slots.is_in_flight(&Arc::new(snapshot.clone()))) {
                yield_now().await;
            }
        })
        .await
        .expect("every pass ends within the timeout");
    }

    #[tokio::test]
    async fn a_second_start_for_a_snapshot_being_prepared_runs_no_second_pass() {
        let slots = slots(4);
        let gate = CancellationToken::new();
        let (start, mut ran) = gated_start(slots.clone(), gate.clone(), Arc::default());

        start.start(&snapshot("a"));
        pretty_assertions::assert_eq!(next_ran(&mut ran).await, snapshot("a"));
        start.start(&snapshot("a"));
        start.start(&snapshot("b"));
        pretty_assertions::assert_eq!(next_ran(&mut ran).await, snapshot("b"));
        gate.cancel();
        until_free(&slots, &[snapshot("a"), snapshot("b")]).await;

        assert!(ran.try_recv().is_err(), "the second start for a ran no pass of its own");
    }

    #[tokio::test]
    async fn two_snapshots_started_in_quick_succession_are_both_prepared() {
        let gate = CancellationToken::new();
        gate.cancel();
        let (start, mut ran) = gated_start(slots(1), gate, Arc::default());

        start.start(&snapshot("a"));
        start.start(&snapshot("b"));

        let mut prepared = [next_ran(&mut ran).await, next_ran(&mut ran).await];
        prepared.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        pretty_assertions::assert_eq!(prepared, [snapshot("a"), snapshot("b")]);
    }

    #[tokio::test]
    async fn a_pass_that_prepared_wakes_status_delivery_and_an_idle_or_failed_one_does_not() {
        let slots = slots(4);
        let gate = CancellationToken::new();
        gate.cancel();
        let wakes = Arc::new(CountedWakes::default());
        let (start, mut ran) = gated_start(slots.clone(), gate, Arc::clone(&wakes));
        let snapshots = [snapshot("prepared"), snapshot("idle"), snapshot("failed")];

        for snapshot in &snapshots {
            start.start(snapshot);
        }
        for _ in &snapshots {
            next_ran(&mut ran).await;
        }
        until_free(&slots, &snapshots).await;

        pretty_assertions::assert_eq!(wakes.count(), 1);
    }

    #[tokio::test]
    async fn stopping_the_tenant_cancels_a_preparation_in_progress_and_waits_for_it() {
        // The process scope is held: dropping it cancels every tenant under it.
        let process = TaskSupervisor::process(Handle::current());
        let supervisor = process.tenant(0);
        let slots = register_snapshot_preparation_slots(&supervisor, NonZeroUsize::MIN).expect("the slots register");
        let wakes = Arc::new(CountedWakes::default());
        let (start, mut ran) = gated_start(slots.clone(), CancellationToken::new(), Arc::clone(&wakes));
        start.start(&snapshot("prepared"));
        pretty_assertions::assert_eq!(next_ran(&mut ran).await, snapshot("prepared"));

        timeout(Duration::from_secs(5), supervisor.shutdown()).await.expect("the tenant shuts down");

        assert!(!slots.is_in_flight(&Arc::new(snapshot("prepared"))), "the pass ended before the tenant stopped");
        pretty_assertions::assert_eq!(wakes.count(), 0, "the pass was cancelled, not run out");
    }
}
