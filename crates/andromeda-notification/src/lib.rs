//! Notification evaluation, payload construction, delivery policy, and durable event and delivery semantics.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

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
/// Recording the outcome of a leased delivery, asked again while its lease can hold.
mod delivery_acknowledgement;
/// Attempt ceiling after which a notification delivery is dead-lettered.
pub mod delivery_attempt_ceiling;
/// Retry wait for a failed notification delivery.
pub mod delivery_backoff;
/// What a claim hands the delivery drain for each delivery it leased.
pub mod delivery_claim;
/// In-flight bound for one notification delivery pass.
pub mod delivery_concurrency;
/// One bounded notification delivery pass.
pub mod delivery_drain;
/// A pending delivery of a notification event to one subscription.
pub mod delivery_entry;
/// Length bound applied to a stored delivery failure diagnostic.
pub mod delivery_error_bound;
/// Worker ownership of a claimed notification delivery.
pub mod delivery_lease;
/// Notification delivery runtime configuration.
pub mod delivery_policy;
/// Whether the queue may still supply due deliveries to a delivery pass.
mod delivery_queue_supply;
/// Notification delivery queue persistence.
pub mod delivery_store;
/// Focused notification failures.
pub mod error;
/// The events a delivery pass's in-flight deliveries already carry.
pub mod held_events;
/// The notified-registration record held in memory for tests.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod in_memory_ledger;
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
/// The ports a notifier is built on, as doubles for its tests.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod notifier_test_double;
/// The committed entity changes handed to live evaluation and not yet
/// evaluated.
pub mod pending_change;
/// Entity event notification evaluation.
pub mod processor;
/// The `@context` a notification is rendered under, and the failure recorded when it cannot be loaded.
mod resolved_notification_context;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration",
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
