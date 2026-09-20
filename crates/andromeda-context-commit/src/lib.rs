//! Atomic context-write policy and the port that persists it.
//!
//! One [`AtomicWrite`] describes the entity mutation and every durable
//! consequence that must share its commit. [`ContextCommitter`] is deliberately
//! one operation: transaction handles, statement phases and backend encodings
//! belong to its implementations.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// Everything one entity write must commit together.
pub mod atomic_write;
/// Deferred temporal and change-event consequences of a batch write.
pub mod batch_side_effect_job;
/// The entity change event consumed after a commit.
pub mod change_event;
/// Commit-time edge notification that deferred consequences are available.
pub mod commit_consequence_publisher;
/// The record and ordinal base a replayed commit continues under.
pub mod commit_replay;
/// The atomic commit port and submission semantics.
pub mod committer;
/// Entity changes paired with their context-space identity.
pub mod context_change_event;
/// An entity captured immediately before deletion.
pub mod deleted_entity;
/// Entity-row mutations and their semantic payload.
pub mod entity_mutation;
/// The current-state row an atomic write stores for one entity.
pub mod entity_record;
/// Failures reported by the atomic commit boundary.
pub mod error;
/// Temporal-history consequences carried by a write.
pub mod history_record;
/// Whether a lease-guarded acknowledgement reached the row it targeted.
pub mod lease_acknowledgement;
/// Preconditions and retry classification for entity mutations.
pub mod precondition;
/// The identity of one durable side-effect job.
pub mod side_effect_job_id;
/// The narrow key a stored Subscription is referenced by.
pub mod subscription_key;
/// Subscriptions a committing write matched.
pub mod subscription_match;
/// The process-wide clock broker-minted version-7 identities come from.
mod uuid_v7_clock;
/// The identity of one atomic broker write.
pub mod write_id;
/// The position of one consequence inside its atomic write.
pub mod write_ordinal;
/// Per-entity atomic write outcomes.
pub mod write_outcome;
/// The identity and ingestion instant a write is replayed under.
pub mod write_record;
/// The attempt budget and backoff a contended write follows.
pub mod write_retry;

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-space",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}
