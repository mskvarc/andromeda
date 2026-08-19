//! Configuration, isolation, identifiers, and lifecycle for integration tests.

#![deny(missing_docs)]

pub mod configuration;
pub mod database_access;
pub mod database_isolation;
pub mod database_reset;
pub mod environment;
pub mod identifiers;
pub mod readiness;
pub mod run_isolated;
pub mod serialized_environment;
pub mod tenant_environment;
pub mod tenant_schema_recreate;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-test-client",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: all,
}
