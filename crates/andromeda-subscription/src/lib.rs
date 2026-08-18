//! Subscription policy for ETSI GS CIM 009 clauses 5.8 and 5.11.

#![deny(missing_docs)]

/// Shared compiled subscription cache.
pub mod cache;
/// The outcome of one matching pass.
pub mod candidate_funnel;
/// The routing index over active subscriptions.
pub mod candidate_index;
#[cfg(test)]
mod candidate_index_property;
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
pub mod expiry;
mod fields;
/// Subscription lifecycle orchestration shared by both subscription kinds.
pub mod lifecycle;
/// In-memory subscription matching policy.
pub mod matching;
/// The reverse map from a subscription to its index buckets.
pub mod membership;
mod normalization;
/// How selectors are placed into the candidate index.
pub mod selector_routing;
/// Spatial-index narrowing of matched subscriptions.
pub mod spatial_narrowing;
/// Subscription lifecycle status transitions.
pub mod status;
/// Subscription persistence ports and write values.
pub mod store;
/// A stored Subscription paired with its surrogate key.
pub mod stored_subscription;
/// Throttling-window arithmetic.
pub mod throttle;
mod validation;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-commit",
        "andromeda-context-registry",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-spatial-index",
        "andromeda-telemetry",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}
