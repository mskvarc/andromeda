//! Named execution and supervision for broker background tasks.
//!
//! This crate owns runtime mechanics only. Policy operations and concrete adapters remain independent and are adapted into these execution forms by broker composition.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

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
        "andromeda-telemetry",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["tokio"],
}

#[cfg(test)]
mod boundary_tests {
    use std::{fs, io::Error, path::Path};

    #[test]
    fn source_names_no_policy_adapter_http_or_database_dependency() -> Result<(), Error> {
        let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))?;
        for forbidden in ["axum", "sqlx", "andromeda-entity", "andromeda-postgresql", "andromeda-delivery"] {
            assert!(!manifest.contains(forbidden), "task runtime must not depend on {forbidden}");
        }
        Ok(())
    }

    #[test]
    fn runtime_never_constructs_an_executor() -> Result<(), Error> {
        let source = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/supervisor.rs"))?;
        assert!(!source.contains("Runtime::new"));
        assert!(!source.contains("Builder::new_multi_thread"));
        assert!(!source.contains("Builder::new_current_thread"));
        Ok(())
    }

    #[test]
    fn policy_crates_remain_independent_of_task_execution_types() -> Result<(), Error> {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for policy in [
            "andromeda-context-registry",
            "andromeda-entity",
            "andromeda-entity-map",
            "andromeda-federation",
            "andromeda-notification",
            "andromeda-registration",
            "andromeda-snapshot",
            "andromeda-subscription",
            "andromeda-temporal",
        ] {
            let root = workspace.join("crates").join(policy);
            let manifest = fs::read_to_string(root.join("Cargo.toml"))?;
            assert!(!manifest.contains("andromeda-task-runtime"), "{policy} must not depend on task runtime");
            assert_policy_sources_are_runtime_neutral(&root.join("src"), policy)?;
        }
        Ok(())
    }

    #[test]
    fn no_legacy_worker_layer_or_policy_runtime_dependency_remains() -> Result<(), Error> {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert!(!workspace.join("src/worker").exists());
        assert!(!workspace.join("crates/andromeda-workers").exists());
        let composition = fs::read_to_string(workspace.join("crates/andromeda-broker-composition/src/tenant_factory/tasks.rs"))?;
        assert!(!composition.contains("DataExpirationWorker"));
        for independent in ["andromeda-entity", "andromeda-postgresql-context-store"] {
            let manifest = fs::read_to_string(workspace.join("crates").join(independent).join("Cargo.toml"))?;
            assert!(!manifest.contains("andromeda-task-runtime"), "{independent} must not depend on task runtime");
        }
        Ok(())
    }

    fn assert_policy_sources_are_runtime_neutral(directory: &Path, policy: &str) -> Result<(), Error> {
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            if path.is_dir() {
                assert_policy_sources_are_runtime_neutral(&path, policy)?;
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let source = fs::read_to_string(&path)?;
                for forbidden in ["TaskRegistration", "TaskSupervisor", "tokio::task::JoinHandle"] {
                    assert!(
                        !source.contains(forbidden),
                        "{policy} policy source must not name {forbidden}: {}",
                        path.display()
                    );
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
