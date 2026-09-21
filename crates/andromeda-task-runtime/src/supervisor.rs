//! Named task supervision on a supplied Tokio execution host.

use crate::{
    criticality::TaskCriticality,
    failure::TaskFailure,
    lifecycle_event::{LifecycleEventKind, TaskLifecycleEvent},
    registration::{TaskRegistration, TaskRegistrationError},
    startup_task::{StartupTask, StartupTaskError},
    task_health::{TaskHealth, TaskHealthSnapshot, TaskHealthState},
    task_name::TaskName,
    task_observer::{DriverTerminal, TerminatedTask, apply_criticality, emit, join_failure, observe_terminal},
};
use andromeda_telemetry::task_failure::record_task_failure;
use dashmap::{DashMap, mapref::entry::Entry};
use std::{
    sync::{
        Arc,
        Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime},
};
use tokio::{
    runtime::Handle,
    sync::{Mutex, Notify, RwLock, broadcast},
    task::{AbortHandle, JoinHandle},
    time::{Instant, sleep},
};
use tokio_util::sync::CancellationToken;

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

    /// Request cooperative cancellation for this scope.
    pub fn request_cancellation(&self) {
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

    /// Return current health for every registered task, ordered by task name.
    pub async fn health_snapshots(&self) -> Vec<TaskHealthSnapshot> {
        let records: Vec<_> = self.inner.health.iter().map(|entry| Arc::clone(entry.value())).collect(); // collect: DashMap guards cannot be held across asynchronous lock acquisition.
        let mut snapshots = Vec::with_capacity(records.len());
        for health in records {
            let mut snapshot = health.read().await.snapshot();
            snapshot.cancellation_requested = self.inner.cancellation.is_cancelled();
            snapshots.push(snapshot);
        }
        snapshots.sort_by(|left, right| left.name.cmp(&right.name));
        snapshots
    }

    /// Run one finite startup task and wait for its result before composition publishes resources.
    ///
    /// # Errors
    ///
    /// Returns [`StartupTaskError`] when the task name is already reserved by this supervisor, when the operation reports a [`TaskFailure`], or when the spawned operation panics or is aborted.
    pub async fn run_startup(&self, task: StartupTask) -> Result<(), StartupTaskError> {
        let (registration, operation) = task.into_parts();
        let health = self.reserve(&registration).map_err(|error| StartupTaskError {
            name: error.name,
            failure: TaskFailure::permanent("duplicate startup task registration"),
        })?;
        let name = registration.name().clone(); // clone: health, lifecycle and the returned error each identify the task independently.
        health.write().await.running(SystemTime::now());
        self.emit(TaskLifecycleEvent::new(name.clone(), LifecycleEventKind::Started)); // clone: the startup result still needs its task name.
        let operation_cancellation = self.inner.cancellation.child_token();
        let result = self.inner.host.spawn(operation(operation_cancellation)).await;
        match result {
            Ok(Ok(())) => {
                health.write().await.terminal(TaskHealthState::Completed);
                self.emit(TaskLifecycleEvent::new(name, LifecycleEventKind::Completed));
                Ok(())
            }
            Ok(Err(failure)) => {
                self.finish_startup_failure(&health, &name, registration.criticality(), &failure).await;
                Err(StartupTaskError { name, failure })
            }
            Err(join_error) => {
                let failure = join_failure(&join_error);
                self.finish_startup_failure(&health, &name, registration.criticality(), &failure).await;
                Err(StartupTaskError { name, failure })
            }
        }
    }

    /// Cooperatively stop every active task and forcibly terminate deadline violators.
    pub async fn shutdown(&self) {
        self.request_cancellation();
        let tasks: Vec<_> = self.inner.active.iter().map(|entry| Arc::clone(entry.value())).collect(); // collect: shutdown awaits without retaining DashMap guards.
        let started = Instant::now();
        loop {
            let mut pending = 0usize;
            let mut nearest_deadline: Option<Duration> = None;
            for task in &tasks {
                if task.health.read().await.is_terminal() {
                    continue;
                }
                pending += 1;
                // A saturated remainder of zero is exactly the expired grace period, so the deadline test stays total.
                let remaining = task.grace_period.saturating_sub(started.elapsed());
                if remaining.is_zero() {
                    task.forced.store(true, Ordering::Release);
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
        for task in tasks {
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
                let health = Arc::new(RwLock::new(TaskHealth::registered(name.clone()))); // clone: the health record and lookup map both identify the task.
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
                Err(error) => DriverTerminal::Failed(join_failure(&error)),
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

    /// Publish a lifecycle event without requiring an attached observer.
    fn emit(&self, event: TaskLifecycleEvent) {
        emit(&self.inner.lifecycle, event);
    }

    /// Record and escalate a failed startup operation.
    async fn finish_startup_failure(&self, health: &RwLock<TaskHealth>, name: &TaskName, criticality: TaskCriticality, failure: &TaskFailure) {
        let mut health = health.write().await;
        health.failed_attempt(SystemTime::now(), failure);
        health.terminal(TaskHealthState::Failed);
        drop(health);
        record_task_failure(failure.class().into(), criticality.into());
        self.emit(TaskLifecycleEvent::new(
            name.clone(), // clone: the startup error retains its own task identity.
            LifecycleEventKind::Failed {
                class: failure.class(),
                summary: failure.shared_summary(),
                criticality,
            },
        ));
        apply_criticality(&self.inner.cancellation, &self.inner.process_cancellation, criticality);
    }
}
