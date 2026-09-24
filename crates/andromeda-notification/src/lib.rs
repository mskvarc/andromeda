//! Notification evaluation, payload construction, delivery policy, and durable event and delivery semantics.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Sending one context source notification.
mod csource_delivery;
/// Context-source interval notification evaluation.
pub mod csource_interval;
/// The registrations a context source registration subscription selects.
mod csource_match_set;
/// Context-source event notification evaluation.
pub mod csource_processor;
/// Attempt ceiling after which a notification delivery is dead-lettered.
pub mod delivery_attempt_ceiling;
/// Retry wait for a failed notification delivery.
pub mod delivery_backoff;
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
/// Entity interval notification evaluation.
pub mod interval;
/// Matching an expanded entity payload against an interval subscription.
mod interval_match;
/// The immutable notification event a write produces.
pub mod notification_event;
/// Entity event notification evaluation.
pub mod processor;
/// Stable ownership identity for a notification delivery lease.
pub mod worker_instance_id;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration-scalars",
        "andromeda-context-space",
        "andromeda-context-commit",
        "andromeda-delivery",
        "andromeda-entity",
        "andromeda-http-headers",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-registration",
        "andromeda-spatial-index",
        "andromeda-subscription",
        "andromeda-telemetry",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
