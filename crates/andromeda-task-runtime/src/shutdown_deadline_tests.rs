use crate::{
    criticality::TaskCriticality,
    failure::TaskFailure,
    registration::{TaskRegistration, TaskRegistrationError},
    resident_task::ResidentTask,
    restart_policy::RestartPolicy,
    runtime_failure::RuntimeFailure,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_health::TaskHealthState,
    task_name::{TaskName, TaskNameError},
};
use pretty_assertions::assert_eq;
use std::{future, time::Duration};
use thiserror::Error;
use tokio::{
    runtime::Handle,
    task::yield_now,
    time::{Instant, advance},
};

#[derive(Debug, Error)]
enum TestError {
    #[error(transparent)]
    Name(#[from] TaskNameError),
    #[error(transparent)]
    Registration(#[from] TaskRegistrationError),
}

/// A task granted `grace` that never observes cancellation.
fn stubborn_task(name: &str, grace: Duration) -> Result<ResidentTask, TestError> {
    let registration = TaskRegistration::new(
        TaskName::try_from(name)?,
        TaskCriticality::Observed,
        RestartPolicy::Never,
        ShutdownPolicy::new(grace),
    );
    Ok(ResidentTask::new(registration, |_| async {
        future::pending::<Result<(), TaskFailure<RuntimeFailure>>>().await
    }))
}

/// A task granted `grace` that stops as soon as it is cancelled.
fn obedient_task(name: &str, grace: Duration) -> Result<ResidentTask, TestError> {
    let registration = TaskRegistration::new(
        TaskName::try_from(name)?,
        TaskCriticality::Observed,
        RestartPolicy::Never,
        ShutdownPolicy::new(grace),
    );
    Ok(ResidentTask::new(registration, |execution| async move {
        execution.cancellation().cancelled().await;
        Ok::<(), TaskFailure<RuntimeFailure>>(())
    }))
}

async fn settle() {
    for _ in 0..8 {
        yield_now().await;
    }
}

/// How long `supervisor` takes to stop by `deadline`, on paused time that
/// jumps to the next timer whenever every task waits.
async fn stop_time(supervisor: &TaskSupervisor, deadline: Instant) -> Duration {
    let started = Instant::now();
    supervisor.shutdown_by(deadline).await;
    started.elapsed()
}

#[tokio::test(start_paused = true)]
async fn a_task_ignoring_cancellation_is_aborted_at_the_deadline_before_its_grace_period_ends() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    supervisor.register_resident(stubborn_task("stubborn", Duration::from_secs(60))?)?;
    settle().await;
    let elapsed = stop_time(&supervisor, Instant::now() + Duration::from_secs(3)).await;
    assert_eq!(elapsed, Duration::from_secs(3));
    let health = supervisor.health(&TaskName::try_from("stubborn")?).await;
    assert_eq!(health.map(|health| health.state), Some(TaskHealthState::ForcedTermination));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn a_task_whose_grace_period_ends_before_the_deadline_is_aborted_at_its_grace_period() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    supervisor.register_resident(stubborn_task("brief", Duration::from_secs(1))?)?;
    settle().await;
    let elapsed = stop_time(&supervisor, Instant::now() + Duration::from_secs(3)).await;
    assert_eq!(elapsed, Duration::from_secs(1));
    Ok(())
}

/// The grace period counts from the cancellation request, so time spent on
/// other work between the request and the shutdown is taken from it.
#[tokio::test(start_paused = true)]
async fn a_grace_period_counts_from_the_cancellation_request_not_from_the_shutdown() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    supervisor.register_resident(stubborn_task("counted", Duration::from_secs(3))?)?;
    settle().await;
    supervisor.request_cancellation();
    advance(Duration::from_secs(2)).await;
    let elapsed = stop_time(&supervisor, Instant::now() + Duration::from_secs(60)).await;
    assert_eq!(elapsed, Duration::from_secs(1));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn a_scope_whose_tasks_stop_when_cancelled_stops_at_once_whatever_its_deadline() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    supervisor.register_resident(obedient_task("obedient", Duration::from_secs(60))?)?;
    settle().await;
    assert_eq!(stop_time(&supervisor, Instant::now() + Duration::from_secs(60)).await, Duration::ZERO);
    let idle = TaskSupervisor::process(Handle::current());
    assert_eq!(stop_time(&idle, Instant::now() + Duration::from_secs(60)).await, Duration::ZERO);
    Ok(())
}

/// A deadline already past aborts every task still running at once.
#[tokio::test(start_paused = true)]
async fn a_deadline_already_past_aborts_every_running_task_at_once() -> Result<(), TestError> {
    let supervisor = TaskSupervisor::process(Handle::current());
    supervisor.register_resident(stubborn_task("late", Duration::from_secs(60))?)?;
    settle().await;
    assert_eq!(stop_time(&supervisor, Instant::now()).await, Duration::ZERO);
    Ok(())
}
