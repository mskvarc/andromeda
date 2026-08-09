//! NGSI-LD temporal lifecycle, query orchestration, mutation policy, and backend-neutral ports.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

mod compilation;
pub mod config;
pub mod error;
pub mod outcome;
pub mod port;
mod read;
pub mod service;
mod write;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-commit",
        "andromeda-entity",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-request",
        "andromeda-ngsi-ld-vocabulary",
    ],
    allow_workspace_dev_dependencies: ["andromeda-ngsi-ld-geometry"],
    allow_production_dependencies: ["confique"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
