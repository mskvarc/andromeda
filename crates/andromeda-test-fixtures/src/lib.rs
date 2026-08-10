//! Canonical NGSI-LD test data and parsed JSON fixture mutation.

#![deny(missing_docs)]

pub mod context;
pub mod entity_fixture;
pub mod fixture_document;
pub mod problem_detail;
pub mod registration_fixture;
pub mod snapshot_fixture;
pub mod subscription_fixture;
pub mod temporal_fixture;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: all,
}
