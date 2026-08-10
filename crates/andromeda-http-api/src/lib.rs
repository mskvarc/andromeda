//! Complete typed assembly of the public Andromeda HTTP route inventory.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod api;
pub mod routes;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-component-report",
        "andromeda-http-component-report",
        "andromeda-http-context-registry",
        "andromeda-http-entity",
        "andromeda-http-entity-map",
        "andromeda-http-health",
        "andromeda-http-registration",
        "andromeda-http-snapshot",
        "andromeda-http-source-identity",
        "andromeda-http-stack",
        "andromeda-http-subscription",
        "andromeda-http-temporal",
        "andromeda-http-tenancy",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-health",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-tenancy",
    ],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
