//! NGSI-LD temporal lifecycle, query orchestration, mutation policy, and backend-neutral ports.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod aggregation_conflict;
/// Eligibility of attributes for aggregation methods (clause 5.7.4.4).
pub mod aggregation_eligibility;
/// The part of an entity a Context Source answered a split temporal query with.
mod answered_evolution;
/// The temporal entities a Context Source answers with, expanded, for tests.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod answered_fixture;
/// The instances a Context Source answered a temporal query with, as history rows.
mod answered_history;
/// How the inputs of one aggregation group disagree in meaning.
pub mod configured_policy;

/// Typed, durable consequences for queued temporal append.
pub mod append_intent;
mod attribute_instance_mutation;
mod compilation;
/// The ways a temporal policy configuration is refused as it is read.
/// Whether Core API writes add to the Temporal Evolution of an Entity.
pub mod delete_by_query;
/// The streams whose older deletions a split query still has to ask its
/// sources for.
pub mod deletion_follow_up;
pub mod error;
mod evolution_write;
/// What a distributed temporal write hands to registrations and keeps locally.
pub mod forwarded_evolution;
/// How many grouping terms nest the aggregates of a read.
mod grouping_depth;
/// The entities a split query's hops reach, read over its window.
mod hop_targets;
/// The instances a split query's filters read in effect when its window
/// opens, merged over every part of an entity.
mod in_effect_entity;
/// The datasets a split query asks its sources about one by one for the
/// instance in effect.
pub mod in_effect_follow_up;
/// The page sizes an instance-returning temporal read runs under.
pub mod instance_limits;
/// Bounded delivery of queued temporal append intents, and the mutation
/// barrier over them.
pub mod intent_drain;
mod intent_preparation;
/// How a rejected temporal append intent is rescheduled and when it is stuck.
pub mod intent_retry_policy;
/// Clause 4.5.23 join over the temporal evolution of Linked Entities.
pub mod linked;
/// What this broker holds in effect of each contributed entity when a split
/// query's window opens.
pub mod local_in_effect;
/// Reading one written attribute instance as the observation a history row records.
pub mod observed_instance;
pub mod outcome;
pub mod port;
/// How the text of one temporal request is read.
pub mod query_reading;
pub mod query_warning_reason;
mod read;
/// What one reified Scope instance of a Temporal Evolution states.
mod reified_scope;
/// The reified Scopes a Temporal Evolution is written with, refused and admitted.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod reified_scope_fixture;
/// The non-local half of Linked Entity resolution for the temporal API.
pub mod remote_linked;
/// Whether the identifiers a request specified narrow a listing of what it matches.
pub mod requested_identifiers;
/// The configured bounds a temporal operation is refused with `TooManyResults` over.
pub mod result_bound;
/// Whether one scope entry is an event or an assertion of the Scope in force.
pub mod scope_entry_kind;
pub mod scope_observation;
pub mod service;
/// The bounds one temporal service applies to what a request may ask of it.
pub mod service_limits;
/// Deciding a split temporal query's filters over the gathered parts of an entity.
pub mod split_settlement;
/// The physical storage policy an operator sets for temporal history.
pub mod storage_policy;
/// A temporal service over one stubbed broker.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod stub_broker;
/// The invariants temporal policy relies on.
pub mod temporal_invariant;
pub mod temporal_not_found_reason;
pub mod temporal_operation_unsupported;
pub mod temporal_validation_reason;
mod unit_code_admission;
/// A mutation refused while its entity's queued history is held elsewhere.
pub mod unsettled_history;
mod write;
/// What one temporal write is for, and what an absent evolution means for it.
pub mod write_purpose;
/// The Temporal Evolution a client writes, from its compact form to the documents its write records.
mod written_evolution;
/// What one instance a written Temporal Evolution states records: an observation or a deletion.
mod written_instance;

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod intent_drain_tests;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration",
        "andromeda-context-commit",
        "andromeda-context-space",
        "andromeda-entity",
        "andromeda-json-literal",
        "andromeda-jsonld",
        "andromeda-linked-retrieval",
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
        "andromeda-retry",
        "andromeda-telemetry",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
