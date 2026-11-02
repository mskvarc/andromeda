//! Process-wide metrics facade for the broker.
//!
//! Every metric the broker emits is named here, recorded through the
//! [`metrics`] facade, and rendered by the one installed [`recorder`]. Nothing
//! in this crate reads PostgreSQL, HTTP or any other backend's types: callers
//! hand it typed names, outcomes and plain values, and the crate turns them into labelled
//! series.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod build_info;
pub mod delivery_ops;
mod description;
pub mod entity;
pub mod error;
pub mod federation_ops;
mod holding_recorder;
pub mod http;
pub mod maintenance;
pub mod name_dictionary;
pub mod names;
pub mod operation;
pub mod pool;
pub mod queue;
pub mod recorder;
mod sample_value;
pub mod statement;
pub mod storage;
pub mod subscription;
pub mod subscription_ops;
pub mod task_failure;
pub mod temporal;
pub mod tenancy;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
