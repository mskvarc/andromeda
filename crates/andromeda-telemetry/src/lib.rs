//! Process-wide metrics facade for the broker.
//!
//! Every metric the broker emits is named here, recorded through the
//! [`metrics`] facade, and rendered by the one installed [`recorder`]. Nothing
//! in this crate knows about PostgreSQL, HTTP or any other backend: callers
//! hand it typed names and outcomes, and the crate turns them into labelled
//! series.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod error;
pub mod maintenance;
pub mod names;
pub mod pool;
pub mod queue;
pub mod recorder;
mod sample_value;
pub mod statement;
pub mod subscription;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
