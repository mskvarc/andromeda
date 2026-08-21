//! NGSI-LD Entity Map lifecycle policy and its replaceable storage port.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// Failures reported by Entity Map policy and storage implementations.
pub mod error;
/// Bounded expiry of Entity Maps past their `expiresAt`.
pub mod expiry;
/// Entity Map lifecycle and intersection semantics.
pub mod service;
/// The replaceable Entity Map storage capability.
pub mod store;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}
