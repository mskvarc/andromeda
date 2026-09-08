//! PostgreSQL statements that read current context information.
//!
//! Clause 3.1 calls an entity's current state *context information*; clauses
//! 5.6.3 and 5.9 define how it and the context source registrations that shadow
//! it are queried. This crate assembles those two `SELECT` statements from the
//! fragments `andromeda-postgresql-query` renders, and does nothing else: it
//! opens no connection, holds no pool, and returns `sea_query` statement trees
//! for the store to bind through `sea-query-sqlx` and execute.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod csr;
pub mod entity;
pub mod entity_relations;
pub mod order_geography;
pub mod snapshot_purge;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-space",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-postgresql-encoding",
        "andromeda-postgresql-query",
        "andromeda-postgresql-schema",
        "andromeda-postgresql-seaquery",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-ngsi-ld-geometry",
    ],
    allow_production_dependencies: ["sea-query"],
}
