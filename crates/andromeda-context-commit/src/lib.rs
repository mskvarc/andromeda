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
/// The invariants an atomic commit relies on.
pub mod commit_invariant;
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
/// The bound every claimable queue holds a recorded failure to.
pub mod failure_summary;
/// Temporal-history consequences carried by a write.
pub mod history_record;
/// Which hoisted geometries of an entity a write changes.
pub mod hoisted_location;
/// The side-effect job lease a replayed commit settles.
pub mod job_lease;
/// Whether a lease-guarded acknowledgement reached the row it targeted.
pub mod lease_acknowledgement;
/// The identity a worker holds a durable queue lease under.
pub mod lease_holder;
/// The clock one pass over a leased queue stamps its releases with.
pub mod lease_pass_clock;
/// Preconditions and retry classification for entity mutations.
pub mod precondition;
/// The attributes a payload carries, with the family of every instance.
pub mod projected_attributes;
/// Whether an entity held a Scope when a write removed it.
pub mod scope_presence;
/// The identity of one durable side-effect job.
pub mod side_effect_job_id;
/// Why the content of a submission cannot be stored as written.
pub mod submission_refusal;
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
        "andromeda-ngsi-ld-entity",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-persistence-failure",
        "andromeda-retry",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}
