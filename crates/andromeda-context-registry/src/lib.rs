//! Hosted and cached JSON-LD Context lifecycle policy and persistence ports.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

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
mod boundary {
    use andromeda_layer_guard::manifest::{assert_layer, baseline_forbidden_except};

    /// Context Registry policy names only domain and JSON-LD capability crates.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// In-workspace crates this policy may name in dependency tables.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-jsonld",
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-model",
            "andromeda-ngsi-ld-vocabulary",
        ];
        let forbidden = baseline_forbidden_except(&[]);
        assert_layer(env!("CARGO_MANIFEST_DIR"), &forbidden, ALLOWED_IN_WORKSPACE);
    }
}
