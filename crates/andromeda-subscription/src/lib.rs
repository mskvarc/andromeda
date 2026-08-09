//! Subscription policy for ETSI GS CIM 009 clauses 5.8 and 5.11.

#![deny(missing_docs)]

/// Shared compiled subscription cache.
pub mod cache;
/// Subscription lifecycle change values.
pub mod change;
/// Subscription compilation policy.
pub mod compilation;
/// Compiled subscription state used by matching paths.
pub mod compiled;
/// Context-source subscription lifecycle policy.
pub mod csource;
/// Compiled context-source-subscription cache.
pub mod csource_cache;
/// Entity subscription lifecycle policy.
pub mod entity;
/// Subscription policy failures.
pub mod error;
mod fields;
/// Subscription lifecycle orchestration shared by both subscription kinds.
pub mod lifecycle;
/// In-memory subscription matching policy.
pub mod matching;
mod normalization;
/// Subscription lifecycle status transitions.
pub mod status;
/// Subscription persistence ports and write values.
pub mod store;
/// Throttling-window arithmetic.
pub mod throttle;
mod validation;

#[cfg(test)]
mod boundary {
    use andromeda_layer_guard::manifest::{assert_layer, baseline_forbidden_except};

    /// Subscription policy remains independent of HTTP runtimes and storage dialects.
    #[test]
    fn this_crate_declares_only_subscription_policy_dependencies() {
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-context-commit",
            "andromeda-context-registry",
            "andromeda-jsonld",
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-geometry",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-model",
            "andromeda-ngsi-ld-query",
            "andromeda-ngsi-ld-representation",
            "andromeda-ngsi-ld-vocabulary",
            "andromeda-spatial-index",
            "andromeda-tenancy",
        ];
        let forbidden = baseline_forbidden_except(&[]);
        assert_layer(env!("CARGO_MANIFEST_DIR"), &forbidden, ALLOWED_IN_WORKSPACE);
    }
}
