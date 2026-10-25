//! Notification evaluation, payload construction, delivery policy, and durable event and delivery semantics.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// The `@context` of a notification whose slot is already claimed.
mod claimed_notification_context;
/// The order context source registrations and subscriptions were written in.
pub mod csource_change_order;
/// Sending one context source notification.
mod csource_delivery;
/// Context-source interval notification evaluation.
pub mod csource_interval;
/// The registrations a context source registration subscription selects.
mod csource_match_set;
/// What each change-driven context source subscription has been told.
mod csource_membership;
/// Context-source event notification evaluation.
pub mod csource_processor;
/// The clause 5.3.3 transitions of a context source notification.
mod csource_transition;
/// Attempt ceiling after which a notification delivery is dead-lettered.
pub mod delivery_attempt_ceiling;
/// Retry wait for a failed notification delivery.
pub mod delivery_backoff;
/// What a claim hands the delivery drain for each delivery it leased.
pub mod delivery_claim;
/// How many more deliveries one delivery pass may claim.
mod delivery_claim_budget;
/// In-flight bound for one notification delivery pass.
pub mod delivery_concurrency;
/// Notification delivery runtime configuration.
pub mod delivery_config;
/// One bounded notification delivery pass.
pub mod delivery_drain;
/// A pending delivery of a notification event to one subscription.
pub mod delivery_entry;
/// Length bound applied to a stored delivery failure diagnostic.
pub mod delivery_error_bound;
/// Worker ownership of a claimed notification delivery.
pub mod delivery_lease;
/// Notification delivery queue persistence.
pub mod delivery_store;
/// Focused notification failures.
pub mod error;
/// The events a delivery pass's in-flight deliveries already carry.
pub mod held_events;
/// Entity interval notification evaluation.
pub mod interval;
/// The entities a periodic notification considers.
mod interval_candidates;
/// Matching an entity state against an interval subscription.
mod interval_match;
/// What one periodic notification carries of the entities it matched.
mod interval_payload;
/// The immutable notification event a write produces.
pub mod notification_event;
/// The invariants notification delivery relies on.
pub mod notification_invariant;
/// The outcome a delivery result records on its subscription.
mod notification_outcome;
/// The registrations each context source subscription was notified of.
pub mod notified_registration_store;
/// Entity event notification evaluation.
pub mod processor;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration-scalars",
        "andromeda-context-commit",
        "andromeda-context-space",
        "andromeda-delivery",
        "andromeda-entity",
        "andromeda-http-headers",
        "andromeda-jsonld",
        "andromeda-linked-retrieval",
        "andromeda-ngsi-ld-compaction",
        "andromeda-ngsi-ld-entity",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-persistence-failure",
        "andromeda-registration",
        "andromeda-retry",
        "andromeda-spatial-index",
        "andromeda-subscription",
        "andromeda-telemetry",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: ["andromeda-http-headers"],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
