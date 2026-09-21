//! Focused HTTP binding and HTTP concern policy for NGSI-LD tenancy.
//!
//! Tenant management is a documented, flag-gated broker extension (ETSI GS CIM
//! 009 v1.9.1 leaves it implementation-specific, clause 5.5.10). This crate binds
//! the [`andromeda_tenancy::administration::TenantAdministration`] port to the
//! `/tenants` routes and turns its domain views into the fixed-JSON `Tenant`
//! resource; no route here ever drops a tenant's physical database.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod error;
pub mod handler;
pub mod options;
pub mod params;
pub mod propagation;
pub mod routes;
pub mod state;
pub mod tenant_response;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-http-extraction",
        "andromeda-http-headers",
        "andromeda-http-response",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-request",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
