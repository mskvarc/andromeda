//! PostgreSQL implementation of the atomic context-commit protocol.
//!
//! The crate owns the transaction that commits current entity state, native
//! PostgreSQL temporal history, notification outbox entries and durable
//! asynchronous side-effect jobs. PostgreSQL statement fusion and projection
//! publication remain private implementation details.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

mod columns;
/// Native PostgreSQL implementation of the context commit port.
pub mod committer;
mod entity_group;
mod history;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-commit",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-postgresql-context-store",
        "andromeda-postgresql-query",
        "andromeda-postgresql-temporal-store",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-ngsi-ld-geometry",
    ],
    allow_production_dependencies: ["sqlx"],
}
