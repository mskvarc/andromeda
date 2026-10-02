//! NGSI-LD temporal lifecycle, query orchestration, mutation policy, and backend-neutral ports.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// How the inputs of one aggregation group disagree in meaning.
pub mod aggregation_conflict;
/// Eligibility of attributes for aggregation methods (clause 5.7.4.4).
pub mod aggregation_eligibility;
/// Typed, durable consequences for deferred temporal append.
pub mod append_intent;
mod attribute_instance_mutation;
mod compilation;
pub mod config;
/// Whether Core API writes add to the Temporal Evolution of an Entity.
pub mod core_write_history;
pub mod delete_by_query;
pub mod error;
mod evolution_write;
/// The page sizes an instance-returning temporal read runs under.
pub mod instance_limits;
/// Choosing the instances of one attribute stream a temporal page keeps.
pub(crate) mod instance_page_selection;
/// Bounded delivery of deferred temporal append intents.
pub mod intent_drain;
mod intent_preparation;
/// How a rejected temporal append intent is rescheduled and when it is stuck.
pub mod intent_retry_policy;
/// Clause 4.5.23 join over the temporal evolution of Linked Entities.
pub mod linked;
/// Reading one written attribute instance as the observation a history row records.
pub mod observed_instance;
/// What one write observed of an entity, as a temporal query selects it by.
pub mod observed_selection;
pub mod outcome;
mod pending_observation;
/// Read overlay combining stored history with committed pending intents.
pub mod pending_overlay;
/// Unfolding the plural members of a written Temporal Evolution into the instances they stand for.
mod plural_member_unfold;
pub mod port;
mod read;
/// What one reified Scope instance of a Temporal Evolution states.
mod reified_scope;
/// The non-local half of Linked Entity resolution for the temporal API.
pub mod remote_linked;
/// Whether the identifiers a request specified narrow a listing of what it matches.
pub mod requested_identifiers;
pub mod scope_observation;
pub mod service;
/// The bounds one temporal service applies to what a request may ask of it.
pub mod service_limits;
/// The physical storage policy an operator sets for temporal history.
pub mod storage_policy;
/// Compacting what one temporal read renders against its request context.
mod temporal_compaction;
/// The invariants temporal policy relies on.
pub mod temporal_invariant;
pub mod temporal_not_found_reason;
pub mod temporal_operation_unsupported;
pub mod temporal_validation_reason;
mod unit_code_admission;
mod write;
/// What one temporal write is for, and what an absent evolution means for it.
pub mod write_purpose;
/// The Temporal Evolution a client writes, from its compact form to the documents its write records.
mod written_evolution;

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
        "andromeda-ngsi-ld-compaction",
        "andromeda-ngsi-ld-entity",
        "andromeda-ngsi-ld-expansion",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-request",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-persistence-failure",
        "andromeda-telemetry",
        "andromeda-unit-catalog",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
