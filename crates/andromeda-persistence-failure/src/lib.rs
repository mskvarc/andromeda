//! The backend-neutral vocabulary of what a store can fail with.
//!
//! Policy crates own ports that persistence adapters implement, and an adapter
//! lives in a crate its port cannot depend on. The adapter's own error (a driver
//! error, a decode fault, a statement that would not build) therefore cannot
//! travel through the port as a typed source. What travels instead is the
//! closed [`PersistenceFailure`](crate::persistence_failure::PersistenceFailure)
//! classification, while the adapter's own error is recorded for operators at
//! the point it is classified.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// The closed classification of a store failure and its operator record.
pub mod persistence_failure;
/// Whether a store failure can be retried unchanged, and why.
pub mod retryability;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
