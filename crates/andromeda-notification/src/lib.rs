//! Notification evaluation, payload construction, delivery policy, and durable outbox semantics.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

/// Context-source interval notification evaluation.
pub mod csource_interval;
/// Context-source event notification evaluation.
pub mod csource_processor;
/// Focused notification failures.
pub mod error;
/// Entity interval notification evaluation.
pub mod interval;
/// Notification outbox runtime configuration.
pub mod outbox_config;
/// One bounded outbox-delivery pass.
pub mod outbox_drain;
/// Durable notification outbox entries.
pub mod outbox_entry;
/// Durable notification outbox persistence.
pub mod outbox_store;
/// Entity event notification evaluation.
pub mod processor;
/// Stable ownership identity for an outbox lease.
pub mod worker_instance_id;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-commit",
        "andromeda-delivery",
        "andromeda-entity",
        "andromeda-http-headers",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-registration",
        "andromeda-spatial-index",
        "andromeda-subscription",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["confique"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
