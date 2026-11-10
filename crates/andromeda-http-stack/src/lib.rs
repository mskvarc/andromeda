//! Explicit scope and ordering for the broker's inbound HTTP middleware.

pub mod cors_tokens;
pub mod stack;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration",
        "andromeda-failure-classification",
        "andromeda-http-extraction",
        "andromeda-http-headers",
        "andromeda-http-response",
        "andromeda-http-tenancy",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-telemetry",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["axum", "tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
