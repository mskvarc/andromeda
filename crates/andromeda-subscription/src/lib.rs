//! Subscription policy for ETSI GS CIM 009 clauses 5.8 and 5.11.

#![deny(missing_docs)]

/// A subscription store answering only reads, for tests.
#[cfg(any(test, feature = "test-support"))]
#[expect(
    clippy::expect_used,
    reason = "test support unwraps and expects on fixtures on purpose; migrating it to typed assertions is pending"
)]
pub mod answering_store;
/// Shared compiled subscription cache.
pub mod cache;
/// The outcome of one matching pass.
pub mod candidate_funnel;
/// The routing index over active subscriptions.
pub mod candidate_index;
#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod candidate_index_property;
/// Subscription lifecycle change values.
pub mod change;
pub mod change_filters;
/// Subscription compilation policy.
pub mod compilation;
/// Compiled subscription state used by matching paths.
pub mod compiled;
/// Cooldown-window arithmetic.
pub mod cooldown;
/// Context-source subscription lifecycle policy.
pub mod csource;
/// Compiled context-source-subscription cache.
pub mod csource_cache;
/// Matching context source registration subscriptions against registrations.
pub mod csource_matching;
/// When a context source registration subscription is owed a notification.
pub mod csource_notification_cadence;
/// The members an update fragment deletes.
pub mod deleted_members;
/// Admitting a subscription's notification endpoint.
pub mod endpoint_admission;
/// Why the delivery bindings refused a notification endpoint.
pub mod endpoint_refusal;
/// Entity subscription lifecycle policy.
pub mod entity;
/// Subscription policy failures.
pub mod error;
pub mod expiry;
mod fields;
/// What the filters a subscription request writes are admitted under.
pub mod filter_admission;
/// The opt-in lists a subscription's `q` filter reads.
mod filter_opt_ins;
/// The distributed side of the subscription lifecycle, as a port.
pub mod forwarding_hook;
/// A subscription's `geoQ`, read once for admission and matching.
pub mod geo_filter;

/// The grid a periodic (`timeInterval`) subscription notifies on.
pub mod interval_schedule;

/// Subscription lifecycle orchestration shared by both subscription kinds.
pub mod lifecycle;
/// In-memory subscription matching policy.
pub mod matching;
/// The reverse map from a subscription to its index buckets.
pub mod membership;
/// The clause 5.2.12 rules a merged update is checked against.
mod merged_validation;
/// Initialising a subscription's notification `@context` from its creation context.
mod notification_context;
/// What a subscription's stored delivery instants must satisfy to notify again.
pub mod notification_slot;
/// The rules a subscription request's notification is checked against.
mod notification_validation;
/// Since when a Subscription may be notified of a change, and which committed
/// changes that leaves owed to it.
pub mod notifying_since;
/// The outcomes of one or more notifications that one delivery-state write
/// records.
pub mod outcome_tally;
/// Expansion of the projection lists of a subscription's notification.
mod projection_expansion;
/// The order in which the reads of one cached subscription take effect.
mod refresh_order;
/// Subscription request validation.
mod request_validation;
/// Spelling a stored subscription in the terms of the reader's `@context`.
mod response_compaction;
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
/// The `@context` a subscription is created or updated under.
pub mod subscription_context;
/// The invariants subscription policy relies on.
pub mod subscription_invariant;
/// One page of a subscription collection, ready for a response.
pub mod subscription_listing;
/// The typed reasons a subscription request is rejected as `BadRequestData`.
pub mod subscription_validation_reason;
/// The selections Subscriptions held before an update replaced them.
mod superseded_selection;
/// Counting a suppression window back from an instant.
mod suppression_window;
/// Expansion of a subscription request against the request `@context`.
mod term_expansion;
/// Throttling-window arithmetic.
pub mod throttle;
/// What one kind of subscription does differently from the other.
pub mod variant;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-commit",
        "andromeda-context-space",
        "andromeda-json-literal",
        "andromeda-jsonld",
        "andromeda-linked-retrieval",
        "andromeda-ngsi-ld-compaction",
        "andromeda-ngsi-ld-entity",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-persistence-failure",
        "andromeda-retry",
        "andromeda-spatial-index",
        "andromeda-telemetry",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-ngsi-ld-number",
        "andromeda-retry",
    ],
    allow_production_dependencies: [],
}
