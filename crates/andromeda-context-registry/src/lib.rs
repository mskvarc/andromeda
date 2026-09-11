//! Hosted and cached JSON-LD Context lifecycle policy and persistence ports.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// The narrow port for periodically invalidating cached contexts.
pub mod cached_context_invalidation;
/// Embedded NGSI-LD core Context representation.
mod core_context;
/// The permitted origins of newly persisted hosted contexts.
pub mod creation_kind;
/// Projection detail requested for Context reads.
pub mod detail_level;
/// Failures reported by Context Registry policy and implementations.
pub mod error;
/// A context fetched through the registry persistence port.
pub mod fetched_context;
/// The narrow port for durably flushing resolver hit deltas.
pub mod hit_store;
/// The identifier that addresses one stored Context.
pub mod identifier;
/// Parameters for listing hosted and cached contexts.
pub mod list_query;
/// A list returned by the registry persistence port.
pub mod listed_contexts;
/// Outcomes of persistence mutations.
pub mod mutation;
/// Delete and reload lifecycle commands.
pub mod removal;
/// Hosted and cached Context lifecycle policy.
pub mod service;
/// The replaceable hosted-context persistence capability.
pub mod store;
/// Stored lifecycle information needed for delete and reload.
pub mod stored_context;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-jsonld",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-vocabulary",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}
