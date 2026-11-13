//! Upgrade rehearsals: a database built at the schema a released version
//! sealed, seeded with the data that version stored, then brought to the schema
//! this build embeds.
//!
//! The broker's own migration sets do the work, so the rehearsal runs the
//! migrations a deployed tenant runs, in the order it runs them. Each rehearsal
//! creates a database of its own on a PostgreSQL server with the `PostGIS`,
//! `TimescaleDB` and `ltree` extensions, so rehearsals run side by side without
//! sharing state. Nothing here reaches a broker over the network.

/// The tenant schema as the v1.0.0 release sealed it.
pub mod release_schema;
/// The database one rehearsal runs in.
pub mod upgrade_database;
/// What can go wrong creating, migrating or dropping a rehearsal database.
pub mod upgrade_database_error;
/// The PostgreSQL server the rehearsals run on.
pub mod upgrade_server;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-space",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-postgresql-branch",
        "andromeda-postgresql-context-store",
        "andromeda-postgresql-pool",
        "andromeda-retry",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-configuration",
        "andromeda-json-literal",
        "andromeda-ngsi-ld-model",
        "andromeda-postgresql-attribute-instance",
        "andromeda-postgresql-seaquery",
        "andromeda-postgresql-temporal-store",
        "andromeda-snapshot",
        "andromeda-tenancy",
    ],
    allow_production_dependencies: ["sqlx"],
}
