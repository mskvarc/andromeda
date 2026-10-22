//! Configuration, isolation, identifiers, and lifecycle for integration tests.

#![deny(missing_docs)]

pub mod broker_process;
pub mod configuration;
pub mod database_access;
pub mod database_catalogue;
pub mod database_isolation;
pub mod database_reset;
pub mod dictionary_seed;
pub mod discovery_census;
pub mod environment;
pub mod identifiers;
pub mod multi_broker;
pub mod pooler_probe;
pub mod readiness;
pub mod resident_tenancy;
pub mod run_isolated;
pub mod scope;
pub mod serialized_environment;
pub mod tenant_environment;
pub mod tenant_lease;
pub mod tenant_provisioning;
pub mod tenant_schema_recreate;
pub mod tenant_scoped_environment;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-lexicon",
        "andromeda-postgresql-pool",
        "andromeda-postgresql-schema",
        "andromeda-postgresql-seaquery",
        "andromeda-tenancy",
        "andromeda-test-client",
        "andromeda-test-fixtures",
        "andromeda-test-mocks",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: all,
}
