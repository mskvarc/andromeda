//! NGSI-LD Entity Map lifecycle policy and its replaceable storage port.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// Failures reported by Entity Map policy and storage implementations.
pub mod error;
/// The built-in in-process Entity Map store.
pub mod memory_store;
/// Entity Map lifecycle and intersection semantics.
pub mod service;
/// The replaceable Entity Map storage capability.
pub mod store;

#[cfg(test)]
mod boundary {
    use andromeda_layer_guard::manifest::{assert_layer, baseline_forbidden_except};

    /// Entity Map policy depends only on its domain model and test-only layer guard.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// In-workspace crates this policy may name in dependency tables.
        const ALLOWED_IN_WORKSPACE: &[&str] = &["andromeda-layer-guard", "andromeda-ngsi-ld-lexicon", "andromeda-ngsi-ld-model"];
        let forbidden = baseline_forbidden_except(&[]);
        assert_layer(env!("CARGO_MANIFEST_DIR"), &forbidden, ALLOWED_IN_WORKSPACE);
    }
}
