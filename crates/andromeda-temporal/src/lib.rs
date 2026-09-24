//! NGSI-LD temporal lifecycle, query orchestration, mutation policy, and backend-neutral ports.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// Typed, durable consequences for deferred temporal append.
/// Eligibility of attributes for aggregation methods (clause 5.7.4.4).
pub mod aggregation_eligibility;
pub mod append_intent;
mod attribute_instance_mutation;
/// The temporal value one expanded attribute instance carries.
pub mod attribute_instance_value;
mod compilation;
pub mod config;
pub mod delete_by_query;
pub mod error;
/// Bounded delivery of deferred temporal append intents.
pub mod intent_drain;
mod intent_preparation;
/// How a rejected temporal append intent is rescheduled and when it is stuck.
pub mod intent_retry_policy;
/// Clause 4.5.23 join over the temporal evolution of Linked Entities.
pub mod linked;
pub mod outcome;
mod pending_observation;
/// Read overlay combining stored history with committed pending intents.
pub mod pending_overlay;
pub mod port;
mod read;
/// The non-local half of Linked Entity resolution for the temporal API.
pub mod remote_linked;
/// Whether the identifiers a request specified narrow a listing of what it matches.
pub mod requested_identifiers;
pub mod scope_observation;
pub mod service;
/// The physical storage policy an operator sets for temporal history.
pub mod storage_policy;
pub mod temporal_diagnostic;
pub mod temporal_not_found_reason;
pub mod temporal_operation_unsupported;
pub mod temporal_validation_reason;
mod unit_code_admission;
mod write;
/// What one temporal write is for, and what an absent evolution means for it.
pub mod write_purpose;

#[cfg(test)]
mod deferred_history_tests;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration-scalars",
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
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
