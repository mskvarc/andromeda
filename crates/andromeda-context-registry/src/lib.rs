//! Hosted and cached JSON-LD Context lifecycle policy and persistence ports.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// The narrow port for periodically invalidating cached contexts.
pub mod cached_context_invalidation;
/// The RFC 8785 canonical form of a stored `@context` document.
mod canonical_document;
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
/// Where a stored Context is served.
pub mod hosted_context_locator;
/// The identifier that addresses one stored Context.
pub mod identifier;
/// Storing the Contexts the broker hosts on its own initiative.
pub mod implicit_context_hosting;
/// Whether a new Context was stored under the identifier it was offered.
pub mod insertion;
/// Which identifier a new Context is stored under.
mod insertion_policy;
/// Parameters for listing hosted and cached contexts.
pub mod list_query;
/// A list returned by the registry persistence port.
pub mod listed_contexts;
/// Outcomes of persistence mutations.
pub mod mutation;
/// Folding un-flushed context accesses into a served `numberOfHits`.
mod pending_hits;
/// The invariants Context Registry policy relies on.
pub mod registry_invariant;
/// Delete and reload lifecycle commands.
pub mod removal;
/// What the Serve @context operation answers with.
pub mod served_context;
/// Hosted and cached Context lifecycle policy.
pub mod service;
/// The replaceable hosted-context persistence capability.
pub mod store;
/// Stored lifecycle information needed for delete and reload.
pub mod stored_context;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-json-literal",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-persistence-failure",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}
