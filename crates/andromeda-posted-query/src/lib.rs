//! Paged queries sent by POST that the broker holds for the page links of
//! their result, and their replaceable storage port.

/// Failures reported by posted-query policy and storage implementations.
pub mod error;
/// Deletion of posted queries past their expiry.
pub mod expiry;
/// A query sent by POST and the digest that names it.
pub mod posted_query;
/// How long a posted query is held, and how many one tenant keeps.
pub mod retention;
/// Holding and resolving posted queries against their store.
pub mod service;
/// The replaceable posted-query storage capability.
pub mod store;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-request",
        "andromeda-persistence-failure",
    ],
    allow_workspace_dev_dependencies: ["andromeda-json-literal"],
    allow_production_dependencies: [],
}
