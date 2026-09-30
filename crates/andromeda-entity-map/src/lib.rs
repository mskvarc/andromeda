//! NGSI-LD Entity Map lifecycle policy and its replaceable storage port.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// What one context source contributed to the Entity Map of a read.
pub mod contribution;
/// The invariants Entity Map policy relies on.
pub mod entity_map_invariant;
/// Failures reported by Entity Map policy and storage implementations.
pub mod error;
/// Bounded expiry of Entity Maps past their `expiresAt`.
pub mod expiry;
/// The canonical digest of a map-creating request's filter parameters.
pub mod fingerprint;
/// Which entities a prepared result page sheds from the Entity Map it ran under.
pub mod pruning;
/// Holding a query's entity selection to the entities a reused map lists.
pub mod query_scoping;
/// Recording what a read did to the Entity Map it ran under.
pub mod recording;
/// What a read does about Entity Maps: reuse, mint, or neither.
pub mod reuse;
/// Entity Map lifecycle and intersection semantics.
pub mod service;
/// The replaceable Entity Map storage capability.
pub mod store;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-request",
        "andromeda-persistence-failure",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}
