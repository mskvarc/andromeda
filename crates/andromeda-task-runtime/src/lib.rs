//! Named execution and supervision for broker background tasks.
//!
//! This crate owns runtime mechanics only. Policy operations and concrete adapters remain independent and are adapted into these execution forms by broker composition.

mod bounded_driver;
/// Cancellation and scheduled-task control handles.
pub mod control;
/// The operational effect of an exhausted task.
pub mod criticality;
/// Pure idle-interval backoff schedule for wake-triggered drains.
pub mod drain_backoff;
mod drain_driver;
mod drain_registration;
/// Wake-triggered durable drain execution.
pub mod drain_task;
mod driver_supervision;
/// Runtime-neutral task failure classification.
pub mod failure;
mod failure_report;
/// Concurrent work that keeps the order of the items sharing a key.
pub mod keyed_lanes;
/// Bounded detached work that runs at most once per key at a time.
pub mod keyed_slots;
mod keyed_slots_registration;
/// Observable task lifecycle events.
pub mod lifecycle_event;
/// Periodic scheduling policy.
pub mod periodic_trigger;
/// Complete metadata required before task execution.
pub mod registration;
mod resident_driver;
mod resident_registration;
/// Long-lived task execution.
pub mod resident_task;
/// Restart decisions after task failures.
pub mod restart_policy;
/// Failures the runtime raises against its own execution contract.
pub mod runtime_failure;
mod scheduled_driver;
mod scheduled_registration;
/// Periodically triggered bounded task execution.
pub mod scheduled_task;
/// Grace periods for cooperative task shutdown.
mod shutdown_cutoff;
pub mod shutdown_policy;
/// Named task supervision on a supplied Tokio execution host.
pub mod supervisor;
/// Observable task health.
pub mod task_health;
/// Validated task identifiers.
pub mod task_name;
mod task_observer;
/// Coalescing wake control and the fallback backoff policy for drains.
pub mod wake_trigger;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-retry",
        "andromeda-telemetry",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["tokio"],
}

#[cfg(test)]
mod shutdown_deadline_tests;
#[cfg(test)]
mod tests;
