use crate::{
    control::RunNowRequest,
    criticality::TaskCriticality,
    drain_task::{DrainPassOutcome, DrainTask},
    failure::TaskFailure,
    lifecycle_event::LifecycleEventKind,
    periodic_trigger::{InitialExecution, MissedTickPolicy, PeriodicSchedule, PeriodicScheduleError},
    registration::{TaskRegistration, TaskRegistrationError},
    resident_task::ResidentTask,
    restart_policy::RestartPolicy,
    scheduled_task::ScheduledTask,
    shutdown_policy::ShutdownPolicy,
    startup_task::{StartupTask, StartupTaskError},
    supervisor::TaskSupervisor,
    task_health::TaskHealthState,
    task_name::{TaskName, TaskNameError},
    wake_trigger::{DrainFallbackPolicy, DrainFallbackPolicyError, WakeRequest, wake_channel},
};
use pretty_assertions::assert_eq;
use std::{
    future,
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use thiserror::Error;
use tokio::{
    runtime::Handle,
    sync::{Notify, broadcast::error::RecvError},
    task::{JoinError, yield_now},
    time::{advance, sleep},
};

#[derive(Debug, Error)]
enum TestError {
    #[error(transparent)]
    Failure(#[from] TaskFailure),
    #[error(transparent)]
    TaskName(#[from] TaskNameError),
    #[error(transparent)]
    Schedule(#[from] PeriodicScheduleError),
    #[error(transparent)]
    Registration(#[from] TaskRegistrationError),
    #[error(transparent)]
    Startup(#[from] StartupTaskError),
    #[error(transparent)]
    Receive(#[from] RecvError),
    #[error(transparent)]
    Join(#[from] JoinError),
    #[error(transparent)]
    DrainPolicy(#[from] DrainFallbackPolicyError),
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
                Ok(())
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
                Ok(())
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
                    Ok(())
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
                Ok(())
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
                Ok(())
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
        future::pending::<Result<(), TaskFailure>>().await
    });
    supervisor.register_resident(task)?;
    settle().await;
    let shutdown_supervisor = supervisor.clone(); // clone: the test observes health while shutdown owns another supervisor handle.
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
            RestartPolicy::transient(1, Duration::from_secs(1), Duration::from_secs(1)),
            Duration::from_secs(1),
        )?,
        move |_| {
            let observed = Arc::clone(&observed);
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(())
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
async fn successful_startup_completion_allows_composition_to_continue() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let task = StartupTask::new(registration("startup-success", RestartPolicy::Never, Duration::from_secs(1))?, |_| async {
        Ok(())
    });
    supervisor.run_startup(task).await?;
    let name = TaskName::try_from("startup-success")?;
    let health = supervisor.health(&name).await;
    assert!(health.is_some_and(|health| health.state == TaskHealthState::Completed));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn failed_startup_is_returned_before_dependent_tasks_start() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    let dependent_started = Arc::new(AtomicBool::new(false));
    let task = StartupTask::new(registration("startup-failure", RestartPolicy::Never, Duration::from_secs(1))?, |_| async {
        Err(TaskFailure::permanent("configuration unavailable"))
    });
    let result = supervisor.run_startup(task).await;
    if result.is_ok() {
        dependent_started.store(true, Ordering::SeqCst);
    }
    assert!(result.is_err());
    assert!(!dependent_started.load(Ordering::SeqCst));
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
            RestartPolicy::transient(3, Duration::from_secs(1), Duration::from_secs(2)),
            Duration::from_secs(1),
        )?,
        move |execution| {
            let observed = Arc::clone(&observed);
            async move {
                let attempt = observed.fetch_add(1, Ordering::SeqCst);
                if attempt < 2 {
                    return Err(TaskFailure::transient("retry"));
                }
                execution.mark_running();
                future::pending::<Result<(), TaskFailure>>().await
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
            RestartPolicy::transient(3, Duration::from_secs(1), Duration::from_secs(2)),
            Duration::from_secs(1),
        )?,
        move |execution| {
            let observed = Arc::clone(&observed);
            async move {
                let attempt = observed.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    return Err(TaskFailure::transient("startup retry"));
                }
                execution.mark_running();
                if attempt == 1 {
                    return Err(TaskFailure::transient("failed after reaching running state"));
                }
                future::pending::<Result<(), TaskFailure>>().await
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
            RestartPolicy::transient(5, Duration::from_secs(1), Duration::from_secs(2)),
            Duration::from_secs(1),
        )?,
        move |_| {
            let observed = Arc::clone(&observed);
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                Err(TaskFailure::permanent("operator action required"))
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
                Ok(())
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
        Err(TaskFailure::invariant_violation("broken tenant invariant"))
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
                Ok(())
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
                Ok(())
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
                Ok(())
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
                        return Ok(DrainPassOutcome::Idle);
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
                Ok(DrainPassOutcome::Idle)
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
                Ok(DrainPassOutcome::Idle)
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
                    Ok(())
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
                Err(TaskFailure::permanent("done"))
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
            | LifecycleEventKind::Completed
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
            RestartPolicy::transient(1, Duration::from_secs(1), Duration::from_secs(1)),
            Duration::from_secs(1),
        )?,
        schedule(Duration::from_secs(60), InitialExecution::Immediate)?,
        move |_| {
            let observed = Arc::clone(&observed);
            async move {
                if observed.fetch_add(1, Ordering::SeqCst) == 0 {
                    Err(TaskFailure::transient("retry once"))
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
    let task = StartupTask::new(registration("shared-host", RestartPolicy::Never, Duration::from_secs(1))?, move |_| {
        let captured = Arc::clone(&captured);
        async move {
            let lock = captured.lock();
            match lock {
                Ok(mut value) => *value = Some(Handle::current().id()),
                Err(_) => return Err(TaskFailure::invariant_violation("test runtime identity lock was poisoned")),
            }
            Ok(())
        }
    });
    supervisor.run_startup(task).await?;
    let lock = observed.lock();
    match lock {
        Ok(value) => assert_eq!(*value, Some(expected)),
        Err(_) => return Err(TaskFailure::invariant_violation("test runtime identity lock was poisoned").into()),
    }
    Ok(())
}
