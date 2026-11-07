//! Complete typed assembly of the public Andromeda HTTP route inventory.

pub mod api;
pub mod management;
pub mod routes;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-component-report",
        "andromeda-http-component-report",
        "andromeda-http-context-registry",
        "andromeda-http-entity",
        "andromeda-http-entity-map",
        "andromeda-http-extraction",
        "andromeda-http-health",
        "andromeda-http-notification-receiver",
        "andromeda-http-registration",
        "andromeda-http-response",
        "andromeda-http-snapshot",
        "andromeda-http-source-identity",
        "andromeda-http-stack",
        "andromeda-http-subscription",
        "andromeda-http-telemetry",
        "andromeda-http-temporal",
        "andromeda-http-tenancy",
        "andromeda-ngsi-ld-request",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-configuration",
        "andromeda-extensions",
        "andromeda-health",
        "andromeda-http-headers",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-telemetry",
        "andromeda-tenancy",
    ],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
