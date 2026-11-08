//! Running one tenant's periodic notification evaluations beside its sweeps.
//!
//! A sweep pass selects the Subscriptions owed a grid point, hands each
//! evaluation (matching read, payload, delivery, outcome) to the tenant's
//! evaluation slots, and returns without waiting for them. A slow endpoint or
//! a crowded pass therefore delays only the Subscriptions involved, and the
//! next pass runs at the next grid point while earlier deliveries are still in
//! flight (clause 5.8.6 notifies each Subscription at its own grid points). A
//! Subscription whose evaluation is still in flight is not started again: its
//! grid point stays owed, and the sweep is woken once the evaluation ends so
//! the next pass picks it up.

use crate::tenant_factory::{assembly_invariant::AssemblyInvariant, error::TenantAssemblyError};
use andromeda_context_commit::subscription_key::SubscriptionKey;
use andromeda_notification::{csource_interval::CsourceIntervalNotificationOperation, error::NotificationError, interval::IntervalNotificationOperation};
use andromeda_retry::schedule::BackoffSchedule;
use andromeda_subscription::stored_subscription::StoredSubscription;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    keyed_slots::KeyedSlots,
    registration::TaskRegistration,
    restart_policy::RestartPolicy,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
};
use chrono::{DateTime, Utc};
use std::{num::NonZeroUsize, sync::Arc, time::Duration};

/// How many periodic evaluations of one tenant run at once, entity and
/// context source Subscriptions together.
const PERIODIC_EVALUATION_WIDTH: NonZeroUsize = NonZeroUsize::MIN.saturating_add(19);

/// One periodic sweep's Subscriptions: which are due, how one is evaluated,
/// and when the next falls due.
pub(super) trait PeriodicEvaluation: Send + Sync + 'static {
    /// One Subscription found due.
    type Due: Send + 'static;

    /// The key `due` holds its evaluation slot under.
    fn key(due: &Self::Due) -> SubscriptionKey;

    /// Every Subscription owed a grid point at `cutoff`.
    fn due(&self, cutoff: DateTime<Utc>) -> impl Future<Output = Result<Vec<Self::Due>, NotificationError>> + Send;

    /// Evaluate `due`, found due at `cutoff`, to its end.
    fn evaluate(&self, due: Self::Due, cutoff: DateTime<Utc>) -> impl Future<Output = ()> + Send;

    /// The earliest grid point of any Subscription after `cutoff`, or `None`
    /// when the sweep has none.
    fn next_due(&self, cutoff: DateTime<Utc>) -> impl Future<Output = Result<Option<DateTime<Utc>>, NotificationError>> + Send;
}

impl PeriodicEvaluation for IntervalNotificationOperation {
    type Due = StoredSubscription;

    fn key(due: &StoredSubscription) -> SubscriptionKey {
        due.key
    }

    fn due(&self, cutoff: DateTime<Utc>) -> impl Future<Output = Result<Vec<StoredSubscription>, NotificationError>> + Send {
        IntervalNotificationOperation::due(self, cutoff)
    }

    fn evaluate(&self, due: StoredSubscription, cutoff: DateTime<Utc>) -> impl Future<Output = ()> + Send {
        IntervalNotificationOperation::evaluate(self, due, cutoff)
    }

    fn next_due(&self, cutoff: DateTime<Utc>) -> impl Future<Output = Result<Option<DateTime<Utc>>, NotificationError>> + Send {
        IntervalNotificationOperation::next_due(self, cutoff)
    }
}

impl PeriodicEvaluation for CsourceIntervalNotificationOperation {
    type Due = StoredSubscription;

    fn key(due: &StoredSubscription) -> SubscriptionKey {
        due.key
    }

    fn due(&self, cutoff: DateTime<Utc>) -> impl Future<Output = Result<Vec<StoredSubscription>, NotificationError>> + Send {
        CsourceIntervalNotificationOperation::due(self, cutoff)
    }

    // The slot a context source notification claims is stamped with the
    // instant it is generated at, not the cutoff it was found due at.
    fn evaluate(&self, due: StoredSubscription, _cutoff: DateTime<Utc>) -> impl Future<Output = ()> + Send {
        CsourceIntervalNotificationOperation::evaluate(self, due)
    }

    fn next_due(&self, cutoff: DateTime<Utc>) -> impl Future<Output = Result<Option<DateTime<Utc>>, NotificationError>> + Send {
        CsourceIntervalNotificationOperation::next_due(self, cutoff)
    }
}

/// One sweep pass at `cutoff`: start the evaluation of every Subscription due
/// whose earlier evaluation has ended, and report when the sweep is next due.
///
/// `rerun` is called once an evaluation this pass found still in flight has
/// ended, so the Subscription's owed grid point is not left waiting for the
/// next grid point of the whole sweep.
///
/// # Errors
///
/// Returns [`NotificationError`] when the due Subscriptions or the next due
/// instant cannot be read.
pub(super) async fn periodic_pass<E, R>(
    evaluation: &Arc<E>,
    slots: &KeyedSlots<SubscriptionKey>,
    rerun: &R,
    cutoff: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, NotificationError>
where
    E: PeriodicEvaluation,
    R: Fn() + Clone + Send + Sync + 'static,
{
    for due in evaluation.due(cutoff).await? {
        let key = E::key(&due);
        let evaluating = Arc::clone(evaluation);
        let rerun = rerun.clone();
        // A Subscription still in flight is refused here; its own evaluation
        // calls `rerun` once it ends.
        let _admission = slots.offer(key, async move { evaluating.evaluate(due, cutoff).await }, rerun);
    }
    evaluation.next_due(cutoff).await
}

/// Create the tenant's periodic evaluation slots, and register the task that
/// stops them, and waits for every evaluation to end, when the tenant shuts
/// down.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the task name is rejected or the
/// supervisor refuses the registration.
pub(super) fn register_periodic_evaluations(supervisor: &TaskSupervisor) -> Result<KeyedSlots<SubscriptionKey>, TenantAssemblyError> {
    let grace = Duration::from_secs(1);
    let name = TaskName::try_from("notification-periodic-evaluations").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(grace)),
        ShutdownPolicy::new(grace),
    );
    supervisor
        .register_keyed_slots(registration, PERIODIC_EVALUATION_WIDTH)
        .map_err(|error| TenantAssemblyError::from(AssemblyInvariant::from(error)))
}

#[cfg(test)]
mod tests {
    use crate::tenant_factory::tasks::periodic_evaluation::{PeriodicEvaluation, periodic_pass, register_periodic_evaluations};
    use andromeda_context_commit::subscription_key::SubscriptionKey;
    use andromeda_notification::error::NotificationError;
    use andromeda_task_runtime::{keyed_slots::KeyedSlots, supervisor::TaskSupervisor};
    use chrono::{DateTime, TimeDelta, Utc};
    use std::{
        future::ready,
        num::NonZeroUsize,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };
    use tokio::{
        runtime::Handle,
        sync::{Notify, mpsc},
        task::yield_now,
        time::timeout,
    };
    use tokio_util::sync::CancellationToken;

    /// The Subscriptions every pass of [`TwoDue`] finds due.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Due {
        First,
        Second,
    }

    impl Due {
        const fn index(self) -> usize {
            match self {
                Due::First => 0,
                Due::Second => 1,
            }
        }
    }

    /// Two Subscriptions, both due at every pass; each evaluation counts
    /// itself, reports that it started, then waits on its Subscription's gate
    /// when it has one.
    struct TwoDue {
        /// How many evaluations of each Subscription started.
        started: [AtomicUsize; 2],
        /// What each Subscription's evaluations wait on before they end.
        gates: [Option<Arc<Notify>>; 2],
        /// Reports each evaluation that started.
        log: mpsc::UnboundedSender<Due>,
    }

    impl TwoDue {
        fn new(gates: [Option<Arc<Notify>>; 2]) -> (Arc<TwoDue>, mpsc::UnboundedReceiver<Due>) {
            let (log, started) = mpsc::unbounded_channel();
            let evaluation = TwoDue {
                started: [AtomicUsize::new(0), AtomicUsize::new(0)],
                gates,
                log,
            };
            (Arc::new(evaluation), started)
        }

        fn started(&self, due: Due) -> usize {
            self.started[due.index()].load(Ordering::Acquire)
        }
    }

    impl PeriodicEvaluation for TwoDue {
        type Due = Due;

        fn key(due: &Due) -> SubscriptionKey {
            key(*due)
        }

        fn due(&self, _cutoff: DateTime<Utc>) -> impl Future<Output = Result<Vec<Due>, NotificationError>> + Send {
            ready(Ok(vec![Due::First, Due::Second]))
        }

        async fn evaluate(&self, due: Due, _cutoff: DateTime<Utc>) {
            self.started[due.index()].fetch_add(1, Ordering::AcqRel);
            self.log.send(due).expect("the test reads the log");
            if let Some(gate) = &self.gates[due.index()] {
                gate.notified().await;
            }
        }

        fn next_due(&self, cutoff: DateTime<Utc>) -> impl Future<Output = Result<Option<DateTime<Utc>>, NotificationError>> + Send {
            ready(Ok(Some(cutoff + TimeDelta::seconds(1))))
        }
    }

    fn key(due: Due) -> SubscriptionKey {
        let raw = match due {
            Due::First => 1,
            Due::Second => 2,
        };
        SubscriptionKey::try_from(raw).expect("a positive key")
    }

    fn slots() -> KeyedSlots<SubscriptionKey> {
        KeyedSlots::new(NonZeroUsize::new(20).expect("a positive width"), CancellationToken::new())
    }

    async fn next_started(log: &mut mpsc::UnboundedReceiver<Due>) -> Due {
        timeout(Duration::from_secs(5), log.recv())
            .await
            .expect("an evaluation starts within the timeout")
            .expect("the evaluation still logs")
    }

    async fn until_free(slots: &KeyedSlots<SubscriptionKey>, due: Due) {
        timeout(Duration::from_secs(5), async {
            while slots.is_in_flight(&key(due)) {
                yield_now().await;
            }
        })
        .await
        .expect("the evaluation ends within the timeout");
    }

    #[tokio::test]
    async fn a_pass_returns_its_next_due_instant_while_one_evaluation_never_ends_and_the_next_pass_evaluates_the_other() {
        let (evaluation, mut log) = TwoDue::new([Some(Arc::new(Notify::new())), None]);
        let slots = slots();
        let cutoff = Utc::now();

        let next = timeout(Duration::from_secs(5), periodic_pass(&evaluation, &slots, &|| {}, cutoff))
            .await
            .expect("the pass does not wait for its evaluations")
            .expect("the pass reads what is due");
        pretty_assertions::assert_eq!(next, Some(cutoff + TimeDelta::seconds(1)));
        let mut first = [next_started(&mut log).await, next_started(&mut log).await];
        first.sort_by_key(|due| due.index());
        pretty_assertions::assert_eq!(first, [Due::First, Due::Second]);
        until_free(&slots, Due::Second).await;

        periodic_pass(&evaluation, &slots, &|| {}, cutoff + TimeDelta::seconds(1))
            .await
            .expect("the pass reads what is due");

        pretty_assertions::assert_eq!(next_started(&mut log).await, Due::Second);
        pretty_assertions::assert_eq!(evaluation.started(Due::First), 1);
    }

    #[tokio::test]
    async fn a_subscription_in_flight_is_not_started_by_a_second_pass_and_is_started_by_the_first_pass_after_it_ends() {
        let gate = Arc::new(Notify::new());
        let (evaluation, mut log) = TwoDue::new([Some(Arc::clone(&gate)), None]);
        let slots = slots();
        let (rerun_sender, mut rerun) = mpsc::unbounded_channel();
        let wake = move || rerun_sender.send(()).expect("the test reads the wakes");
        let cutoff = Utc::now();

        periodic_pass(&evaluation, &slots, &wake, cutoff).await.expect("the pass reads what is due");
        let _ = [next_started(&mut log).await, next_started(&mut log).await];
        until_free(&slots, Due::Second).await;
        periodic_pass(&evaluation, &slots, &wake, cutoff + TimeDelta::seconds(1))
            .await
            .expect("the pass reads what is due");
        pretty_assertions::assert_eq!(next_started(&mut log).await, Due::Second);
        pretty_assertions::assert_eq!(evaluation.started(Due::First), 1);

        gate.notify_one();
        timeout(Duration::from_secs(5), rerun.recv())
            .await
            .expect("the sweep is woken once the evaluation ends")
            .expect("the wake still reports");
        assert!(!slots.is_in_flight(&key(Due::First)), "the key is free before the sweep is woken");
        until_free(&slots, Due::Second).await;
        periodic_pass(&evaluation, &slots, &wake, cutoff + TimeDelta::seconds(2))
            .await
            .expect("the pass reads what is due");

        let mut third = [next_started(&mut log).await, next_started(&mut log).await];
        third.sort_by_key(|due| due.index());
        pretty_assertions::assert_eq!(third, [Due::First, Due::Second]);
        gate.notify_one();
    }

    #[tokio::test]
    async fn shutting_the_tenant_down_stops_every_evaluation_in_flight() {
        // The process scope is held: dropping it cancels every tenant under it.
        let process = TaskSupervisor::process(Handle::current());
        let supervisor = process.tenant(0);
        let slots = register_periodic_evaluations(&supervisor).expect("the task registers");
        let (evaluation, mut log) = TwoDue::new([Some(Arc::new(Notify::new())), Some(Arc::new(Notify::new()))]);
        periodic_pass(&evaluation, &slots, &|| {}, Utc::now())
            .await
            .expect("the pass reads what is due");
        let _ = [next_started(&mut log).await, next_started(&mut log).await];
        assert!(
            slots.is_in_flight(&key(Due::First)) && slots.is_in_flight(&key(Due::Second)),
            "both evaluations wait on their gates"
        );

        timeout(Duration::from_secs(5), supervisor.shutdown()).await.expect("the tenant shuts down");

        assert!(
            !slots.is_in_flight(&key(Due::First)) && !slots.is_in_flight(&key(Due::Second)),
            "nothing the evaluations started outlives the tenant"
        );
    }
}
