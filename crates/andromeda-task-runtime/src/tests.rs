use crate::{
    control::{DrainCause, DrainExecution, RunNowRequest},
    criticality::TaskCriticality,
    drain_task::{DrainPassOutcome, DrainTask},
    failure::{FailureClass, TaskFailure},
    lifecycle_event::LifecycleEventKind,
    periodic_trigger::{InitialExecution, MissedTickPolicy, PeriodicSchedule, PeriodicScheduleError},
    registration::{TaskRegistration, TaskRegistrationError},
    resident_task::ResidentTask,
    restart_policy::RestartPolicy,
    scheduled_task::ScheduledTask,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_health::TaskHealthState,
    task_name::{TaskName, TaskNameError},
    wake_trigger::{DrainFallbackPolicy, WakeRequest, wake_channel},
};
use andromeda_retry::{growth::Growth, schedule::BackoffSchedule, schedule_error::BackoffScheduleError};
use pretty_assertions::assert_eq;
use std::{
    future,
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};
use thiserror::Error;
use tokio::{
    runtime::Handle,
    sync::{Notify, broadcast::error::RecvError},
    task::{JoinError, yield_now},
    time::{advance, sleep},
};

/// The typed failure a scripted test operation reports.
#[derive(Debug, Error)]
enum ScriptedFailure {
    #[error("retry")]
    Retry,
    #[error("failed after reaching running state")]
    FailedAfterRunning,
    #[error("operator action required")]
    OperatorActionRequired,
    #[error("broken tenant invariant")]
    BrokenTenantInvariant,
    #[error("done")]
    Done,
    #[error("test runtime identity lock was poisoned")]
    IdentityLockPoisoned,
}

/// What a scripted test operation reports.
type Scripted<T> = Result<T, TaskFailure<ScriptedFailure>>;

#[derive(Debug, Error)]
enum TestError {
    #[error(transparent)]
    Failure(#[from] TaskFailure<ScriptedFailure>),
    #[error(transparent)]
    TaskName(#[from] TaskNameError),
    #[error(transparent)]
    Schedule(#[from] PeriodicScheduleError),
    #[error(transparent)]
    Registration(#[from] TaskRegistrationError),
    #[error(transparent)]
    Receive(#[from] RecvError),
    #[error(transparent)]
    Join(#[from] JoinError),
    #[error(transparent)]
    DrainPolicy(#[from] BackoffScheduleError),
}

fn registration(name: &str, restart_policy: RestartPolicy, grace: Duration) -> Result<TaskRegistration, TestError> {
    Ok(TaskRegistration::new(
        TaskName::try_from(name)?,
        TaskCriticality::Observed,
        restart_policy,
        ShutdownPolicy::new(grace),
    ))
}

fn schedule(cadence: Duration, initial: InitialExecution) -> Result<PeriodicSchedule, TestError> {
    Ok(PeriodicSchedule::new(cadence, initial, MissedTickPolicy::Skip)?)
}

async fn settle() {
    for _ in 0..8 {
        yield_now().await;
    }
}

#[tokio::test(start_paused = true)]
async fn immediate_scheduled_execution_obeys_the_first_cadence() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let executions = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&executions);
    let task = ScheduledTask::new(
        registration("immediate", RestartPolicy::Never, Duration::from_secs(1))?,
        schedule(Duration::from_secs(60), InitialExecution::Immediate)?,
        move |_| {
            let observed = Arc::clone(&observed);
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                Scripted::Ok(())
            }
        },
    );
    drop(supervisor.register_scheduled(task)?);
    settle().await;
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    advance(Duration::from_secs(59)).await;
    settle().await;
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    advance(Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(executions.load(Ordering::SeqCst), 2);
    supervisor.shutdown().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn delayed_scheduled_execution_waits_one_complete_cadence() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let executions = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&executions);
    let task = ScheduledTask::new(
        registration("delayed", RestartPolicy::Never, Duration::from_secs(1))?,
        schedule(Duration::from_secs(60), InitialExecution::AfterCadence)?,
        move |_| {
            let observed = Arc::clone(&observed);
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                Scripted::Ok(())
            }
        },
    );
    drop(supervisor.register_scheduled(task)?);
    settle().await;
    assert_eq!(executions.load(Ordering::SeqCst), 0);
    advance(Duration::from_secs(60)).await;
    settle().await;
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    supervisor.shutdown().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn elapsed_ticks_coalesce_without_concurrent_execution() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let executions = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(AtomicUsize::new(0));
    let maximum_active = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(Notify::new());
    let task = ScheduledTask::new(
        registration("coalescing", RestartPolicy::Never, Duration::from_secs(1))?,
        schedule(Duration::from_secs(60), InitialExecution::Immediate)?,
        {
            let executions = Arc::clone(&executions);
            let active = Arc::clone(&active);
            let maximum_active = Arc::clone(&maximum_active);
            let release = Arc::clone(&release);
            move |_| {
                let executions = Arc::clone(&executions);
                let active = Arc::clone(&active);
                let maximum_active = Arc::clone(&maximum_active);
                let release = Arc::clone(&release);
                async move {
                    let call = executions.fetch_add(1, Ordering::SeqCst);
                    let now_active = active.fetch_add(1, Ordering::SeqCst) + 1;
                    maximum_active.fetch_max(now_active, Ordering::SeqCst);
                    if call == 0 {
                        release.notified().await;
                    }
                    active.fetch_sub(1, Ordering::SeqCst);
                    Scripted::Ok(())
                }
            }
        },
    );
    drop(supervisor.register_scheduled(task)?);
    settle().await;
    advance(Duration::from_secs(180)).await;
    settle().await;
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    release.notify_waiters();
    settle().await;
    assert_eq!(executions.load(Ordering::SeqCst), 2);
    assert_eq!(maximum_active.load(Ordering::SeqCst), 1);
    supervisor.shutdown().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn idle_scheduled_task_cancels_without_waiting_for_its_cadence() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let executions = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&executions);
    let task = ScheduledTask::new(
        registration("idle-cancel", RestartPolicy::Never, Duration::from_secs(1))?,
        schedule(Duration::from_secs(3_600), InitialExecution::AfterCadence)?,
        move |_| {
            let observed = Arc::clone(&observed);
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                Scripted::Ok(())
            }
        },
    );
    drop(supervisor.register_scheduled(task)?);
    settle().await;
    supervisor.shutdown().await;
    assert_eq!(executions.load(Ordering::SeqCst), 0);
    let name = TaskName::try_from("idle-cancel")?;
    let health = supervisor.health(&name).await;
    assert!(health.is_some_and(|health| health.state == TaskHealthState::Cancelled));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn active_operation_can_return_its_lease_before_cancellation_completes() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let lease_returned = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&lease_returned);
    let task = ScheduledTask::new(
        registration("lease-return", RestartPolicy::Never, Duration::from_secs(10))?,
        schedule(Duration::from_secs(60), InitialExecution::Immediate)?,
        move |execution| {
            let observed = Arc::clone(&observed);
            async move {
                execution.cancellation().cancelled().await;
                observed.store(true, Ordering::SeqCst);
                Scripted::Ok(())
            }
        },
    );
    drop(supervisor.register_scheduled(task)?);
    settle().await;
    supervisor.shutdown().await;
    assert!(lease_returned.load(Ordering::SeqCst));
    let name = TaskName::try_from("lease-return")?;
    let health = supervisor.health(&name).await;
    assert!(health.is_some_and(|health| health.state == TaskHealthState::Cancelled));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn shutdown_forces_a_task_that_ignores_its_deadline() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let task = ResidentTask::new(registration("forced", RestartPolicy::Never, Duration::from_secs(10))?, |_| async {
        future::pending::<Result<(), TaskFailure<ScriptedFailure>>>().await
    });
    supervisor.register_resident(task)?;
    settle().await;
    let shutdown_supervisor = supervisor.clone();
    let shutdown = tokio::spawn(async move { shutdown_supervisor.shutdown().await });
    settle().await;
    advance(Duration::from_secs(10)).await;
    settle().await;
    shutdown.await?;
    let name = TaskName::try_from("forced")?;
    let health = supervisor.health(&name).await;
    assert!(health.is_some_and(|health| health.state == TaskHealthState::ForcedTermination));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn unexpected_resident_completion_consults_restart_policy() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&attempts);
    let task = ResidentTask::new(
        registration(
            "resident-complete",
            RestartPolicy::transient(1, BackoffSchedule::new(Duration::from_secs(1), Duration::from_secs(1), Growth::Doubling)),
            Duration::from_secs(1),
        )?,
        move |_| {
            let observed = Arc::clone(&observed);
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                Scripted::Ok(())
            }
        },
    );
    supervisor.register_resident(task)?;
    settle().await;
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    advance(Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    let name = TaskName::try_from("resident-complete")?;
    let health = supervisor.health(&name).await;
    assert!(health.is_some_and(|health| health.state == TaskHealthState::Failed));
    supervisor.shutdown().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn transient_restarts_follow_one_and_two_second_backoff() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&attempts);
    let task = ResidentTask::new(
        registration(
            "resident-backoff",
            RestartPolicy::transient(3, BackoffSchedule::new(Duration::from_secs(1), Duration::from_secs(2), Growth::Doubling)),
            Duration::from_secs(1),
        )?,
        move |execution| {
            let observed = Arc::clone(&observed);
            async move {
                let attempt = observed.fetch_add(1, Ordering::SeqCst);
                if attempt < 2 {
                    return Err(TaskFailure::transient(ScriptedFailure::Retry));
                }
                execution.mark_running();
                future::pending::<Result<(), TaskFailure<ScriptedFailure>>>().await
            }
        },
    );
    supervisor.register_resident(task)?;
    settle().await;
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    advance(Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    advance(Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    advance(Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    let name = TaskName::try_from("resident-backoff")?;
    let health = supervisor.health(&name).await;
    assert!(health.is_some_and(|health| health.state == TaskHealthState::Running && health.consecutive_failures == 0));
    supervisor.request_cancellation();
    let shutdown = supervisor.shutdown();
    tokio::pin!(shutdown);
    advance(Duration::from_secs(1)).await;
    shutdown.await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn resident_readiness_resets_failures_before_an_immediate_later_failure() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&attempts);
    let task = ResidentTask::new(
        registration(
            "resident-readiness-reset",
            RestartPolicy::transient(3, BackoffSchedule::new(Duration::from_secs(1), Duration::from_secs(2), Growth::Doubling)),
            Duration::from_secs(1),
        )?,
        move |execution| {
            let observed = Arc::clone(&observed);
            async move {
                let attempt = observed.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    return Err(TaskFailure::transient(ScriptedFailure::Retry));
                }
                execution.mark_running();
                if attempt == 1 {
                    return Err(TaskFailure::transient(ScriptedFailure::FailedAfterRunning));
                }
                future::pending::<Result<(), TaskFailure<ScriptedFailure>>>().await
            }
        },
    );
    supervisor.register_resident(task)?;
    settle().await;
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    advance(Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    advance(Duration::from_secs(1)).await;
    settle().await;
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    supervisor.request_cancellation();
    let shutdown = supervisor.shutdown();
    tokio::pin!(shutdown);
    advance(Duration::from_secs(1)).await;
    shutdown.await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn permanent_failure_does_not_restart() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&attempts);
    let task = ResidentTask::new(
        registration(
            "permanent",
            RestartPolicy::transient(5, BackoffSchedule::new(Duration::from_secs(1), Duration::from_secs(2), Growth::Doubling)),
            Duration::from_secs(1),
        )?,
        move |_| {
            let observed = Arc::clone(&observed);
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                Err(TaskFailure::permanent(ScriptedFailure::OperatorActionRequired))
            }
        },
    );
    supervisor.register_resident(task)?;
    settle().await;
    advance(Duration::from_secs(30)).await;
    settle().await;
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    supervisor.shutdown().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn a_task_stopped_for_good_is_listed_with_its_criticality_and_failure() -> Result<(), TestError> {
    let process = TaskSupervisor::process(Handle::current());
    let supervisor = process.tenant(1);
    let degrading = TaskRegistration::new(
        TaskName::try_from("degrading")?,
        TaskCriticality::TenantDegrading,
        RestartPolicy::Never,
        ShutdownPolicy::new(Duration::from_secs(1)),
    );
    supervisor.register_resident(ResidentTask::new(degrading, |_| async {
        Err(TaskFailure::permanent(ScriptedFailure::OperatorActionRequired))
    }))?;
    supervisor.register_resident(ResidentTask::new(
        registration("healthy", RestartPolicy::Never, Duration::from_secs(1))?,
        |execution| async move {
            execution.cancellation().cancelled().await;
            Scripted::Ok(())
        },
    ))?;
    settle().await;
    advance(Duration::from_secs(1)).await;
    settle().await;
    let failed = supervisor.failed_tasks().await;
    assert_eq!(failed.len(), 1);
    let task = &failed[0];
    assert_eq!(task.name.as_str(), "degrading");
    assert_eq!(task.state, TaskHealthState::Failed);
    assert_eq!(task.criticality, TaskCriticality::TenantDegrading);
    assert_eq!(task.last_failure_class, Some(FailureClass::Permanent));
    assert_eq!(task.last_failure_summary.as_deref(), Some("operator action required"));
    assert!(!supervisor.cancellation_token().is_cancelled());
    supervisor.shutdown().await;
    assert_eq!(supervisor.failed_tasks().await.len(), 1);
    drop(process);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn tenant_fatal_invariant_cancels_siblings_but_not_process_scope() -> Result<(), TestError> {
    let process = TaskSupervisor::process(Handle::current());
    let tenant = process.tenant(1);
    let sibling_cancelled = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&sibling_cancelled);
    tenant.register_resident(ResidentTask::new(
        registration("sibling", RestartPolicy::Never, Duration::from_secs(1))?,
        move |execution| {
            let observed = Arc::clone(&observed);
            async move {
                execution.cancellation().cancelled().await;
                observed.store(true, Ordering::SeqCst);
                Scripted::Ok(())
            }
        },
    ))?;
    let fatal_registration = TaskRegistration::new(
        TaskName::try_from("fatal")?,
        TaskCriticality::TenantFatal,
        RestartPolicy::Never,
        ShutdownPolicy::new(Duration::from_secs(1)),
    );
    tenant.register_resident(ResidentTask::new(fatal_registration, |_| async {
        Err(TaskFailure::invariant_violation(ScriptedFailure::BrokenTenantInvariant))
    }))?;
    settle().await;
    assert!(tenant.cancellation_token().is_cancelled());
    assert!(sibling_cancelled.load(Ordering::SeqCst));
    assert!(!process.cancellation_token().is_cancelled());
    tenant.shutdown().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn tenant_cancellation_is_isolated_from_sibling_tenant() -> Result<(), TestError> {
    let process = TaskSupervisor::process(Handle::current());
    let first = process.tenant(1);
    let second = process.tenant(2);
    for (name, supervisor) in [("first-tenant", &first), ("second-tenant", &second)] {
        supervisor.register_resident(ResidentTask::new(
            registration(name, RestartPolicy::Never, Duration::from_secs(1))?,
            |execution| async move {
                execution.cancellation().cancelled().await;
                Scripted::Ok(())
            },
        ))?;
    }
    settle().await;
    first.request_cancellation();
    settle().await;
    assert!(first.cancellation_token().is_cancelled());
    assert!(!second.cancellation_token().is_cancelled());
    first.shutdown().await;
    second.shutdown().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn process_cancellation_reaches_process_and_all_tenant_tasks() -> Result<(), TestError> {
    let process = TaskSupervisor::process(Handle::current());
    let first = process.tenant(1);
    let second = process.tenant(2);
    for (name, supervisor) in [("process-task", &process), ("tenant-one", &first), ("tenant-two", &second)] {
        supervisor.register_resident(ResidentTask::new(
            registration(name, RestartPolicy::Never, Duration::from_secs(1))?,
            |execution| async move {
                execution.cancellation().cancelled().await;
                Scripted::Ok(())
            },
        ))?;
    }
    process.request_cancellation();
    settle().await;
    assert!(process.cancellation_token().is_cancelled());
    assert!(first.cancellation_token().is_cancelled());
    assert!(second.cancellation_token().is_cancelled());
    first.shutdown().await;
    second.shutdown().await;
    process.shutdown().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn run_now_signals_coalesce_into_one_prompt_execution() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let executions = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&executions);
    let task = ScheduledTask::new(
        registration("run-now", RestartPolicy::Never, Duration::from_secs(1))?,
        schedule(Duration::from_secs(3_600), InitialExecution::AfterCadence)?,
        move |_| {
            let observed = Arc::clone(&observed);
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                Scripted::Ok(())
            }
        },
    );
    let run_now = supervisor.register_scheduled(task)?;
    assert_eq!(run_now.request(), RunNowRequest::Scheduled);
    assert_eq!(run_now.request(), RunNowRequest::Coalesced);
    settle().await;
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    supervisor.shutdown().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn drain_wake_runs_one_bounded_pass_and_acknowledges_only_after_processing() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let durable_items = Arc::new(AtomicUsize::new(1));
    let sink_effects = Arc::new(AtomicUsize::new(0));
    let acknowledgements = Arc::new(AtomicUsize::new(0));
    let task = DrainTask::new(
        registration("drain-wake", RestartPolicy::Never, Duration::from_secs(1))?,
        DrainFallbackPolicy::new(Duration::from_secs(3_600), Duration::from_secs(3_600))?,
        {
            let durable_items = Arc::clone(&durable_items);
            let sink_effects = Arc::clone(&sink_effects);
            let acknowledgements = Arc::clone(&acknowledgements);
            move |_| {
                let durable_items = Arc::clone(&durable_items);
                let sink_effects = Arc::clone(&sink_effects);
                let acknowledgements = Arc::clone(&acknowledgements);
                async move {
                    if durable_items.load(Ordering::SeqCst) == 0 {
                        return Scripted::Ok(DrainPassOutcome::Idle);
                    }
                    assert_eq!(acknowledgements.load(Ordering::SeqCst), 0);
                    sink_effects.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(sink_effects.load(Ordering::SeqCst), 1);
                    durable_items.store(0, Ordering::SeqCst);
                    acknowledgements.fetch_add(1, Ordering::SeqCst);
                    Ok(DrainPassOutcome::Idle)
                }
            }
        },
    );
    let (wake, receiver) = wake_channel();
    supervisor.register_drain(task, receiver)?;
    settle().await;
    assert_eq!(sink_effects.load(Ordering::SeqCst), 0);
    assert_eq!(wake.wake(), WakeRequest::Scheduled);
    settle().await;
    assert_eq!(sink_effects.load(Ordering::SeqCst), 1);
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 1);
    supervisor.shutdown().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn an_idle_drain_runs_at_most_six_passes_in_thirty_seconds() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let passes = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&passes);
    let task = DrainTask::new(
        registration("idle-backoff", RestartPolicy::Never, Duration::from_secs(1))?,
        DrainFallbackPolicy::new(Duration::from_secs(1), Duration::from_secs(8))?,
        move |_| {
            let observed = Arc::clone(&observed);
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                Scripted::Ok(DrainPassOutcome::Idle)
            }
        },
    );
    let (_wake, receiver) = wake_channel();
    supervisor.register_drain(task, receiver)?;
    // Under a paused clock the runtime auto-advances to each drain timer, so this
    // sleep lets exactly the passes due within thirty seconds run.
    sleep(Duration::from_secs(30)).await;
    settle().await;
    let count = passes.load(Ordering::SeqCst);
    assert!(
        (1..=6).contains(&count),
        "an idle drain backs off to at most six passes in thirty seconds, ran {count}"
    );
    supervisor.shutdown().await;
    Ok(())
}

/// A pass knows what started it, so a composer can treat a producer's wake
/// and its follow-up passes differently from the fallback timer.
#[tokio::test(start_paused = true)]
async fn every_drain_pass_reports_whether_a_wake_its_predecessor_or_the_fallback_started_it() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let causes = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&causes);
    let task = DrainTask::new(
        registration("drain-causes", RestartPolicy::Never, Duration::from_secs(1))?,
        DrainFallbackPolicy::new(Duration::from_secs(60), Duration::from_secs(60))?,
        move |execution: DrainExecution| {
            let observed = Arc::clone(&observed);
            async move {
                let mut causes = observed.lock().expect("cause log");
                causes.push(execution.cause());
                // The woken pass reports more work, so exactly one continuation follows it.
                Scripted::Ok(if causes.len() == 1 {
                    DrainPassOutcome::WorkMayRemain
                } else {
                    DrainPassOutcome::Idle
                })
            }
        },
    );
    let (wake, receiver) = wake_channel();
    supervisor.register_drain(task, receiver)?;
    settle().await;
    assert_eq!(wake.wake(), WakeRequest::Scheduled);
    settle().await;
    sleep(Duration::from_secs(61)).await;
    settle().await;
    assert_eq!(
        causes.lock().expect("cause log").clone(),
        vec![DrainCause::Wake, DrainCause::Continuation, DrainCause::Fallback]
    );
    supervisor.shutdown().await;
    Ok(())
}

/// A pass that reported when its next work falls due is followed, once that
/// instant comes, by a pass that knows it is due; a pass its pacer rested is
/// followed by an ordinary fallback pass.
#[tokio::test(start_paused = true)]
async fn the_pass_ending_a_reported_due_sleep_is_due_and_the_one_ending_a_rest_is_a_fallback() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let causes = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&causes);
    let task = DrainTask::new(
        registration("due-causes", RestartPolicy::Never, Duration::from_secs(1))?,
        DrainFallbackPolicy::new(Duration::from_secs(1), Duration::from_secs(60))?,
        move |execution: DrainExecution| {
            let observed = Arc::clone(&observed);
            async move {
                let mut causes = observed.lock().expect("cause log");
                causes.push(execution.cause());
                let resume = SystemTime::now() + Duration::from_millis(500);
                Scripted::Ok(match causes.len() {
                    1 => DrainPassOutcome::IdleUntil(resume),
                    2 => DrainPassOutcome::RestUntil(resume),
                    _ => DrainPassOutcome::Idle,
                })
            }
        },
    );
    let (wake, receiver) = wake_channel();
    supervisor.register_drain(task, receiver)?;
    settle().await;
    assert_eq!(wake.wake(), WakeRequest::Scheduled);
    settle().await;
    // Each scripted sleep is 500 ms; the idle backoff after the third pass is
    // the 1 s floor, so 1.5 s covers exactly three passes.
    sleep(Duration::from_millis(1_500)).await;
    settle().await;
    assert_eq!(
        causes.lock().expect("cause log").clone(),
        vec![DrainCause::Wake, DrainCause::Due, DrainCause::Fallback]
    );
    supervisor.shutdown().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn a_wake_sent_before_registration_is_not_lost() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let passes = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&passes);
    let task = DrainTask::new(
        registration("early-wake", RestartPolicy::Never, Duration::from_secs(1))?,
        // A long fallback: without the retained wake, no pass would run for an hour.
        DrainFallbackPolicy::new(Duration::from_secs(3_600), Duration::from_secs(3_600))?,
        move |_| {
            let observed = Arc::clone(&observed);
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                Scripted::Ok(DrainPassOutcome::Idle)
            }
        },
    );
    let (wake, receiver) = wake_channel();
    // The wake is sent before the task is registered; the one-slot channel retains it.
    assert_eq!(wake.wake(), WakeRequest::Scheduled);
    supervisor.register_drain(task, receiver)?;
    settle().await;
    assert_eq!(passes.load(Ordering::SeqCst), 1, "the wake sent before registration drove one prompt pass");
    supervisor.shutdown().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn missed_cadences_need_no_persisted_timer_jobs_for_level_triggered_work() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let due = Arc::new(AtomicUsize::new(0));
    let processed = Arc::new(AtomicUsize::new(0));
    let enabled = Arc::new(AtomicBool::new(false));
    let task = ScheduledTask::new(
        registration("level-trigger", RestartPolicy::Never, Duration::from_secs(1))?,
        schedule(Duration::from_secs(60), InitialExecution::Immediate)?,
        {
            let due = Arc::clone(&due);
            let processed = Arc::clone(&processed);
            let enabled = Arc::clone(&enabled);
            move |_| {
                let due = Arc::clone(&due);
                let processed = Arc::clone(&processed);
                let enabled = Arc::clone(&enabled);
                async move {
                    if enabled.load(Ordering::SeqCst) {
                        processed.fetch_add(due.swap(0, Ordering::SeqCst), Ordering::SeqCst);
                    }
                    Scripted::Ok(())
                }
            }
        },
    );
    drop(supervisor.register_scheduled(task)?);
    settle().await;
    due.store(3, Ordering::SeqCst);
    advance(Duration::from_secs(180)).await;
    settle().await;
    assert_eq!(processed.load(Ordering::SeqCst), 0);
    enabled.store(true, Ordering::SeqCst);
    advance(Duration::from_secs(60)).await;
    settle().await;
    assert_eq!(processed.load(Ordering::SeqCst), 3);
    supervisor.shutdown().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn terminal_events_are_observed_in_completion_order_with_task_names() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let mut events = supervisor.subscribe();
    for (name, delay) in [("third", 3), ("second", 2), ("first", 1)] {
        supervisor.register_resident(ResidentTask::new(
            registration(name, RestartPolicy::Never, Duration::from_secs(1))?,
            move |_| async move {
                sleep(Duration::from_secs(delay)).await;
                Err(TaskFailure::permanent(ScriptedFailure::Done))
            },
        ))?;
    }
    settle().await;
    advance(Duration::from_secs(3)).await;
    settle().await;
    let mut terminal_names = Vec::new();
    while terminal_names.len() < 3 {
        let event = events.recv().await?;
        match event.kind {
            LifecycleEventKind::Failed {
                class: _,
                summary: _,
                criticality: _,
            }
            | LifecycleEventKind::Cancelled
            | LifecycleEventKind::ForcedTermination => terminal_names.push(event.name),
            LifecycleEventKind::Started
            | LifecycleEventKind::ExecutionSucceeded { duration: _ }
            | LifecycleEventKind::BackingOff {
                class: _,
                summary: _,
                delay: _,
            } => {}
        }
    }
    let names: Vec<_> = terminal_names.iter().map(TaskName::as_str).collect();
    assert_eq!(names, vec!["first", "second", "third"]);
    supervisor.shutdown().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn health_transitions_from_backoff_to_success_and_resets_failures() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&attempts);
    let task = ScheduledTask::new(
        registration(
            "health-transition",
            RestartPolicy::transient(1, BackoffSchedule::new(Duration::from_secs(1), Duration::from_secs(1), Growth::Doubling)),
            Duration::from_secs(1),
        )?,
        schedule(Duration::from_secs(60), InitialExecution::Immediate)?,
        move |_| {
            let observed = Arc::clone(&observed);
            async move {
                if observed.fetch_add(1, Ordering::SeqCst) == 0 {
                    Err(TaskFailure::transient(ScriptedFailure::Retry))
                } else {
                    Ok(())
                }
            }
        },
    );
    drop(supervisor.register_scheduled(task)?);
    settle().await;
    let name = TaskName::try_from("health-transition")?;
    let backing_off = supervisor.health(&name).await;
    assert!(
        backing_off
            .is_some_and(|health| { health.state == TaskHealthState::BackingOff && health.consecutive_failures == 1 && health.backoff_deadline.is_some() })
    );
    advance(Duration::from_secs(1)).await;
    settle().await;
    let running = supervisor.health(&name).await;
    assert!(running.is_some_and(|health| {
        health.state == TaskHealthState::Running
            && health.consecutive_failures == 0
            && health.last_success_at.is_some()
            && health.last_success_duration.is_some()
    }));
    supervisor.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn managed_tasks_run_on_the_supplied_primary_execution_host() -> Result<(), TestError> {
    let host = Handle::current();
    let expected = host.id();
    let supervisor = TaskSupervisor::process(host);
    let observed = Arc::new(Mutex::new(None));
    let captured = Arc::clone(&observed);
    let ran = Arc::new(Notify::new());
    let announce = Arc::clone(&ran);
    let task = ScheduledTask::new(
        registration("shared-host", RestartPolicy::Never, Duration::from_secs(1))?,
        schedule(Duration::from_secs(3_600), InitialExecution::Immediate)?,
        move |_| {
            let captured = Arc::clone(&captured);
            let announce = Arc::clone(&announce);
            async move {
                let lock = captured.lock();
                match lock {
                    Ok(mut value) => *value = Some(Handle::current().id()),
                    Err(_) => return Err(TaskFailure::invariant_violation(ScriptedFailure::IdentityLockPoisoned)),
                }
                announce.notify_one();
                Ok(())
            }
        },
    );
    drop(supervisor.register_scheduled(task)?);
    ran.notified().await;
    let lock = observed.lock();
    match lock {
        Ok(value) => assert_eq!(*value, Some(expected)),
        Err(_) => return Err(TaskFailure::invariant_violation(ScriptedFailure::IdentityLockPoisoned).into()),
    }
    Ok(())
}
