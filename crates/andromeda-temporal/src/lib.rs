//! NGSI-LD temporal lifecycle, query orchestration, mutation policy, and backend-neutral ports.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// Typed, durable consequences for deferred temporal append.
/// Eligibility of attributes for aggregation methods (clause 5.7.4.4).
pub mod aggregation_eligibility;
pub mod append_intent;
mod compilation;
pub mod config;
pub mod error;
/// Bounded delivery of deferred temporal append intents.
pub mod intent_drain;
mod intent_preparation;
/// How a rejected temporal append intent is rescheduled and when it is stuck.
pub mod intent_retry_policy;
/// Clause 4.5.23 join over the temporal evolution of Linked Entities.
pub mod linked;
pub mod outcome;
/// Read overlay combining stored history with committed pending intents.
pub mod pending_overlay;
pub mod port;
mod read;
/// The non-local half of Linked Entity resolution for the temporal API.
pub mod remote_linked;
pub mod scope_observation;
pub mod service;
/// The physical storage policy an operator sets for temporal history.
pub mod storage_policy;
mod unit_code_admission;
mod write;

#[cfg(test)]
mod deferred_history_tests;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-space",
        "andromeda-context-commit",
        "andromeda-entity",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-request",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-telemetry",
    ],
    allow_workspace_dev_dependencies: ["andromeda-ngsi-ld-geometry"],
    allow_production_dependencies: ["confique"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
