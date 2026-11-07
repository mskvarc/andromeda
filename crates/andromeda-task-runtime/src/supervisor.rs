//! Named task supervision on a supplied Tokio execution host.

use crate::{
    failure_report::FailureReport,
    lifecycle_event::TaskLifecycleEvent,
    registration::{TaskRegistration, TaskRegistrationError},
    shutdown_cutoff::ShutdownCutoff,
    task_health::{TaskHealth, TaskHealthSnapshot},
    task_name::TaskName,
    task_observer::{DriverTerminal, TerminatedTask, join_failure, observe_terminal},
};
use dashmap::{DashMap, mapref::entry::Entry};
use std::{
    sync::{
        Arc,
        OnceLock,
        Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    runtime::Handle,
    sync::{Mutex, Notify, RwLock, broadcast},
    task::{AbortHandle, JoinHandle},
    time::{Instant, sleep},
};
use tokio_util::sync::CancellationToken;
use tracing::warn;

/// Cloneable process- or tenant-scoped supervisor handle.
#[derive(Clone)]
pub struct TaskSupervisor {
    /// Shared supervisor resources and task registry.
    pub(crate) inner: Arc<SupervisorInner>,
}

/// The default per-tenant spread applied to a scheduled task that sets no jitter
/// of its own, capped at half each task's cadence.
///
/// A fleet of tenant runtimes built in one provisioning burst would otherwise run
/// every shared background task in lockstep, stampeding the database in periodic
/// waves; a per-tenant seed spreads each task's phase across this window instead.
const TENANT_STARTUP_JITTER: Duration = Duration::from_secs(10);

/// Resources shared by every clone of one supervisor.
pub(crate) struct SupervisorInner {
    /// Primary process execution host supplied by composition.
    pub(crate) host: Handle,
    /// Cancellation root for this process or tenant scope.
    pub(crate) cancellation: CancellationToken,
    /// When cancellation of this scope was first requested; every task's
    /// grace period counts from it.
    cancellation_requested_at: OnceLock<Instant>,
    /// Process root used only for process-fatal escalation.
    process_cancellation: CancellationToken,
    /// Seed folded into each scheduled task's jitter so this scope's tasks do not
    /// fire in lockstep with the same task in another scope. Zero for the process
    /// scope, which keeps its singleton tasks on their exact cadence.
    pub(crate) scope_seed: u64,
    /// Default jitter given to a scheduled task in this scope that sets none of
    /// its own. Zero for the process scope.
    pub(crate) scope_default_jitter: Duration,
    /// Names reserved for the lifetime of this supervisor.
    registered: DashMap<TaskName, ()>,
    /// Health records retained after task termination.
    health: DashMap<TaskName, Arc<RwLock<TaskHealth>>>,
    /// Active task handles and shutdown deadlines.
    active: DashMap<TaskName, Arc<ManagedTask>>,
    /// Completion-ordered lifecycle reporting channel.
    pub(crate) lifecycle: broadcast::Sender<TaskLifecycleEvent>,
    /// Wake-up for graceful shutdown progress.
    completion: Notify,
}

impl Drop for SupervisorInner {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

/// Handles and shutdown metadata for one running task.
struct ManagedTask {
    /// Shared health record used to detect terminal state.
    health: Arc<RwLock<TaskHealth>>,
    /// Capability used only after cooperative shutdown expires.
    abort: AbortHandle,
    /// Completion observer awaited during explicit shutdown.
    observer: Mutex<Option<JoinHandle<()>>>,
    /// Distinguishes deadline abort from an unexpected driver abort.
    forced: Arc<AtomicBool>,
    /// Task-specific cooperative shutdown duration.
    grace_period: Duration,
}

impl TaskSupervisor {
    /// Construct a process supervisor on the supplied primary Tokio runtime.
    #[must_use]
    pub fn process(host: Handle) -> TaskSupervisor {
        let process_cancellation = CancellationToken::new();
        TaskSupervisor::with_tokens(host, process_cancellation.clone(), process_cancellation, 0, Duration::ZERO)
    }

    /// Construct an isolated tenant supervisor whose cancellation is a child of this process scope.
    ///
    /// `jitter_seed` distinguishes this tenant's scheduled tasks from the same
    /// tasks in every other tenant so a provisioning burst does not stampede the
    /// database with synchronized background work; pass a stable per-tenant value.
    #[must_use]
    pub fn tenant(&self, jitter_seed: u64) -> TaskSupervisor {
        TaskSupervisor::with_tokens(
            self.inner.host.clone(), // clone: process and tenant supervisors spawn on the same runtime handle.
            self.inner.process_cancellation.child_token(),
            self.inner.process_cancellation.clone(), // clone: tenant-fatal cancellation stays local while process-fatal cancellation reaches the root.
            jitter_seed,
            TENANT_STARTUP_JITTER,
        )
    }

    /// Construct one supervisor over explicit scope and process tokens.
    fn with_tokens(
        host: Handle,
        cancellation: CancellationToken,
        process_cancellation: CancellationToken,
        scope_seed: u64,
        scope_default_jitter: Duration,
    ) -> TaskSupervisor {
        let (lifecycle, _) = broadcast::channel(256);
        TaskSupervisor {
            inner: Arc::new(SupervisorInner {
                host,
                cancellation,
                cancellation_requested_at: OnceLock::new(),
                process_cancellation,
                scope_seed,
                scope_default_jitter,
                registered: DashMap::new(),
                health: DashMap::new(),
                active: DashMap::new(),
                lifecycle,
                completion: Notify::new(),
            }),
        }
    }

    /// Subscribe to lifecycle transitions in the order they are observed.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<TaskLifecycleEvent> {
        self.inner.lifecycle.subscribe()
    }

    /// Return the cancellation token for the supervisor scope.
    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.inner.cancellation.clone() // clone: callers receive another handle to the same hierarchical cancellation state.
    }

    /// Request cooperative cancellation for this scope. The first request
    /// starts every task's grace period.
    pub fn request_cancellation(&self) {
        self.inner.cancellation_requested_at.get_or_init(Instant::now);
        self.inner.cancellation.cancel();
    }

    /// Return one task's current health, when registered.
    pub async fn health(&self, name: &TaskName) -> Option<TaskHealthSnapshot> {
        let health = self.inner.health.get(name).map(|entry| Arc::clone(entry.value())); // clone: the lock must outlive the DashMap guard.
        let health = health?;
        let mut snapshot = health.read().await.snapshot();
        snapshot.cancellation_requested = self.inner.cancellation.is_cancelled();
        Some(snapshot)
    }

    /// Return every task that stopped for good, after its failures outlasted its restart policy.
    ///
    /// A task that observed cancellation or was aborted at its shutdown deadline did not fail
    /// and is not listed. The snapshots sort by task name.
    pub async fn failed_tasks(&self) -> Vec<TaskHealthSnapshot> {
        let records: Vec<_> = self.inner.health.iter().map(|entry| Arc::clone(entry.value())).collect(); // collect: the locks are awaited without retaining DashMap guards.
        let mut failed = Vec::new();
        for record in records {
            let snapshot = record.read().await.snapshot();
            if snapshot.state.is_failed() {
                failed.push(snapshot);
            }
        }
        failed.sort_by(|left, right| left.name.cmp(&right.name));
        failed
    }

    /// Cooperatively stop every active task, and abort each one still
    /// running when its grace period, counted from the first cancellation
    /// request, runs out.
    pub async fn shutdown(&self) {
        self.stop(None).await;
    }

    /// [`shutdown`](Self::shutdown), and abort every task still running at
    /// `deadline`, however much of its grace period is left.
    pub async fn shutdown_by(&self, deadline: Instant) {
        self.stop(Some(deadline)).await;
    }

    /// Stop every active task within its grace period and, when given, by
    /// `deadline`, logging each task cut off and the bound that cut it.
    async fn stop(&self, deadline: Option<Instant>) {
        self.request_cancellation();
        let started = *self.inner.cancellation_requested_at.get_or_init(Instant::now);
        let tasks: Vec<_> = self.inner.active.iter().map(|entry| (entry.key().clone(), Arc::clone(entry.value()))).collect(); // collect: shutdown awaits without retaining DashMap guards; clone: each task is named in the log of its cutoff.
        loop {
            let mut pending = 0usize;
            let mut nearest_deadline: Option<Duration> = None;
            for (name, task) in &tasks {
                if task.health.read().await.is_terminal() {
                    continue;
                }
                pending += 1;
                // A saturated remainder of zero is exactly the expired bound, so the deadline test stays total.
                let (remaining, cutoff) = ShutdownCutoff::remaining(
                    task.grace_period.saturating_sub(started.elapsed()),
                    deadline.map(|deadline| deadline.saturating_duration_since(Instant::now())),
                );
                if remaining.is_zero() {
                    if !task.forced.swap(true, Ordering::AcqRel) {
                        warn!(event = "task_shutdown_cut_off", task = %name, cutoff = %cutoff, "a stopping task was aborted before it stopped on its own");
                    }
                    task.abort.abort();
                } else {
                    nearest_deadline = Some(nearest_deadline.map_or(remaining, |nearest| nearest.min(remaining)));
                }
            }
            if pending == 0 {
                break;
            }
            let delay = nearest_deadline.unwrap_or(Duration::from_millis(1));
            tokio::select! {
                () = self.inner.completion.notified() => {}
                () = sleep(delay) => {}
            }
        }
        for (_, task) in tasks {
            let observer = task.observer.lock().await.take();
            if let Some(observer) = observer
                && let Err(error) = observer.await
            {
                tracing::error!(%error, "task observer failed during shutdown");
            }
        }
    }

    /// Reserve one task name and create its registered health record.
    pub(crate) fn reserve(&self, registration: &TaskRegistration) -> Result<Arc<RwLock<TaskHealth>>, TaskRegistrationError> {
        let name = registration.name().clone(); // clone: registration remains complete while the supervisor owns the lookup key.
        match self.inner.registered.entry(name.clone()) {
            Entry::Occupied(_) => Err(TaskRegistrationError { name }),
            Entry::Vacant(entry) => {
                entry.insert(());
                let health = Arc::new(RwLock::new(TaskHealth::registered(name.clone(), registration.criticality()))); // clone: the health record and lookup map both identify the task.
                self.inner.health.insert(name, Arc::clone(&health)); // clone: task execution and health queries share the same record.
                Ok(health)
            }
        }
    }

    /// Spawn an observer that attributes every terminal driver result.
    pub(crate) fn observe(&self, name: TaskName, registration: &TaskRegistration, health: Arc<RwLock<TaskHealth>>, driver: JoinHandle<DriverTerminal>) {
        let abort = driver.abort_handle();
        let forced = Arc::new(AtomicBool::new(false));
        let observer_forced = Arc::clone(&forced); // clone: shutdown sets the flag while the observer classifies cancellation.
        let observer_health = Arc::clone(&health); // clone: terminal observation updates the registered health record.
        let inner = Arc::downgrade(&self.inner);
        let observer_name = name.clone(); // clone: the active registry keeps the same task identity.
        let criticality = registration.criticality();
        let grace_period = registration.shutdown_policy().grace_period();
        let observer = self.inner.host.spawn(async move {
            let terminal = match driver.await {
                Ok(terminal) => terminal,
                Err(error) => DriverTerminal::Failed(FailureReport::of(&join_failure(error))),
            };
            if let Some(inner) = Weak::upgrade(&inner) {
                observe_terminal(
                    &inner.cancellation,
                    &inner.process_cancellation,
                    &inner.lifecycle,
                    &inner.completion,
                    TerminatedTask {
                        name: observer_name,
                        criticality,
                        health: observer_health,
                        forced: observer_forced,
                    },
                    terminal,
                )
                .await;
            }
        });
        self.inner.active.insert(
            name,
            Arc::new(ManagedTask {
                health,
                abort,
                observer: Mutex::new(Some(observer)),
                forced,
                grace_period,
            }),
        );
    }
}
