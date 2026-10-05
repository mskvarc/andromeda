//! Entity policy for ETSI GS CIM 009 clauses 5.6 and 5.7.

#![deny(missing_docs)]

/// Admitting write payloads the way the transport layer does, for tests.
#[cfg(test)]
mod admission_fixture;
/// A single-entity write document the clause 5.5.4 admission produced.
pub mod admitted_write;
/// What the existing Entities carry of one attribute name.
pub mod attribute_summary;
/// Attribute-level writes (clauses 5.6.2 to 5.6.5 and 5.6.19).
pub mod attrs;
/// Batch entity operations and execution policy.
pub mod batch;
/// The one Building subscription the durable-match tests match against.
#[cfg(test)]
mod building_subscription_fixture;
/// The notifications a committed change to current state owes, whichever API made it.
pub mod change_notifier;
/// Publication boundary for committed entity changes.
pub mod change_sink;
/// The change events a side-effect job publishes once the replay commit that settled it has landed.
mod committed_side_effect;
/// Refusing a typed write whose geometry is past the position budget before it is expanded.
pub mod compact_geometry_admission;
/// A Building store another writer changes mid-operation, for the compare-and-set tests.
#[cfg(test)]
mod compare_and_set_fixture;
/// Cache of compiled entity queries, owned by the query service.
pub mod compiled_cache;
/// Entity execution and background-operation configuration.
pub mod config;
/// What a deletion needs to know about the entity it removes, short of its payload.
pub mod deletion_outline;
/// A deletion-outline reader answering from a fixture state, for tests.
#[cfg(test)]
mod deletion_outline_fixture;
/// Entity type and attribute discovery operations.
pub mod discovery;
/// Counting what a written document costs against the payload budget.
pub mod document_census;
/// The clause 5.5.4 admission run through the entity service.
#[cfg(test)]
mod entity_admission_tests;
/// The invariants entity policy relies on.
pub mod entity_invariant;
/// The concrete reasons an entity operation is unsupported by the storage backend.
pub mod entity_operation_unsupported;
/// Versioned entity snapshots used by optimistic writes.
pub mod entity_snapshot;
/// Whether a stored entity is the one a clause 4.17 type selector names.
mod entity_type_match;
/// What the existing Entities carrying one type hold.
pub mod entity_type_summary;
/// The concrete reasons an entity payload or operation is rejected as invalid.
pub mod entity_validation_reason;
/// Entity policy failures.
pub mod error;
/// Applying entity-level and attribute-instance expiry policy.
pub mod expiry;
/// Assertions over the Temporal Evolution a recorded write carries.
#[cfg(test)]
mod history_recording_assertions;
/// Coalescing concurrent writers by entity identifier.
pub mod id_lock;
/// An entity service reading the in-memory store, for tests.
#[cfg(test)]
mod in_memory_service;
/// An in-memory store applying write preconditions, for tests.
#[cfg(test)]
mod in_memory_store;
/// The fragment rewrite a merge performs for the target's attribute types.
pub mod merge_fragment;
/// The Table 6.5.3.4-1 options of a merge.
pub mod merge_options;
/// The write that stores a modified entity, and the notifications it owes.
mod modification_write;
/// Whole-entity mutations (clauses 5.6.1 to 5.6.6).
pub mod mutation;
/// Compare-and-set tests for Delete Entity.
#[cfg(test)]
mod mutation_deletion_compare_and_set;
/// Payload-read, match and tombstone tests for Delete Entity.
#[cfg(test)]
mod mutation_deletion_recording;
/// Durable-match recording tests for the whole-entity merge and replace paths.
#[cfg(test)]
mod mutation_durable_match_recording;
/// History-recording tests for the whole-entity merge and replace paths.
#[cfg(test)]
mod mutation_history_recording;
/// Turning one mutation into the subscriptions it matched.
pub mod notification_fanout;
/// Planning both notification channels from one mutation's per-trigger groups.
pub mod notification_plan;
/// A recording notification harness shared by the durable-match tests.
#[cfg(test)]
mod notification_recording_harness;
/// Store-applied or policy-applied pagination state.
pub mod pagination_outcome;
/// Caller-shaped ports consumed by entity policy.
pub mod port;
/// Purging attributes from a set of entities.
pub mod purge;
/// Entity query and retrieval operations.
pub mod query;
/// Query rows and unapplied work returned by a store.
pub mod query_outcome;
/// Structural representation of predicates left for policy evaluation.
pub mod query_residual;
/// Turning a read plus a computation into an optimistic write.
pub mod read_modify_write;
/// The ports a recording entity service is wired to.
#[cfg(test)]
mod recording_ports;
/// Evaluating predicates a store declined to apply.
pub mod residual_filter;
/// One named query predicate a store may decline.
pub mod residual_predicate;
/// Work a batch write produces but need not finish before answering.
pub mod side_effect;
/// Bounded at-least-once drain for durable entity side effects.
pub mod side_effect_drain;
/// Settling the notifications a drained side-effect job still owes.
pub mod side_effect_notification;
/// The durable side-effect queue port.
pub mod side_effect_queue;
/// The writes a drained side-effect job replays.
mod side_effect_replay;
/// The entity state a matched subscription is filtered and notified with.
mod snapshot_supply;
/// Which spatial index, if any, a geo query may be narrowed through.
pub mod spatial_acceleration;
/// Entity spatial-index consistency configuration.
pub mod spatial_config;
/// Keeping the entity spatial index in step with every live commit.
pub mod spatial_indexing_committer;
/// Where one entity's location belongs in the entity spatial index.
pub mod spatial_placement;
/// Rebuilding the entity spatial index from a read of the store.
pub mod spatial_reconciliation;
/// Catching the entity spatial index up from the store before it narrows a query.
pub mod spatial_watermark;
/// Every write a broker process commits is visible to its next geo query.
#[cfg(test)]
mod spatial_write_visibility;
/// Query capabilities of one entity store.
pub mod storage_profile;
/// The clause 5.5.4 screening a write payload passes before anything else.
pub mod write_admission;
/// The write operations an admitted payload is bound to, as types.
pub mod write_kind;
/// The members a written Entity Fragment names, as the attribute names
/// Update Attributes and Append Attributes report.
mod written_members;

use crate::{
    batch::execution::BatchExecutionConfig,
    change_notifier::ChangeNotifier,
    id_lock::{IdLockMap, acquire_id_locks_in},
    port::{
        attribute_history::AttributeDatasetHistory,
        blocking_executor::BlockingExecutor,
        deletion_outline::DeletionOutlineReader,
        reader::EntityReader,
        retry_delay::RetryDelay,
        service_ports::EntityServicePorts,
    },
    side_effect::{AsyncPreference, SideEffectPlacement, resolve_side_effect_placement},
};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_representation::payload_budget::EntityPayloadBudget;
use andromeda_subscription::cache::SubscriptionCache;
use async_lock::MutexGuardArc;
use dashmap::DashMap;
use std::sync::Arc;

/// Configuration for durable notification-event recording at commit time.
#[derive(Clone, Debug)]
pub struct NotificationWriteConfig {
    /// Shared subscription cache for matching entity mutations against subscriptions.
    pub subscription_cache: Arc<SubscriptionCache>,
}

/// Policy for NGSI-LD entity CRUD and batch operations.
pub struct EntityService {
    reader: Arc<dyn EntityReader>,
    blocking_executor: Arc<dyn BlockingExecutor>,
    retry_delay: Arc<dyn RetryDelay>,
    attribute_history: Arc<dyn AttributeDatasetHistory>,
    writes: Arc<dyn ContextCommitter>,
    engine: JsonLdEngine,
    notifier: Arc<ChangeNotifier>,
    batch_execution: BatchExecutionConfig,
    payload_budget: EntityPayloadBudget,
    deletion_outline: Arc<dyn DeletionOutlineReader>,
    id_locks: Arc<IdLockMap>,
}

impl EntityService {
    /// Construct entity policy over caller-owned ports and runtime bindings.
    ///
    /// `notifier` is shared with every other policy that changes current
    /// state, so a change is notified the same way whichever API made it.
    #[must_use]
    pub fn new(
        ports: EntityServicePorts,
        engine: JsonLdEngine,
        notifier: Arc<ChangeNotifier>,
        batch_config: BatchExecutionConfig,
        payload_budget: EntityPayloadBudget,
    ) -> EntityService {
        let EntityServicePorts {
            reader,
            blocking_executor,
            retry_delay,
            attribute_history,
            writes,
            deletion_outline,
        } = ports;
        let id_locks: Arc<IdLockMap> = Arc::new(DashMap::new());
        let batch_execution = batch_config.clamped_to_usable_limits();

        EntityService {
            reader,
            blocking_executor,
            retry_delay,
            attribute_history,
            writes,
            engine,
            notifier,
            batch_execution,
            payload_budget,
            deletion_outline,
            id_locks,
        }
    }

    /// Acquire owned locks for every distinct identifier in sorted order.
    pub(crate) async fn acquire_id_locks(&self, target: &ContextTarget, ids: &[Identifier]) -> Vec<MutexGuardArc<()>> {
        acquire_id_locks_in(&self.id_locks, target, ids).await
    }

    /// Place one batch of `batch_size` entities against `target`: where it
    /// writes, and whether its side effects run inline or are queued for the
    /// side-effect worker.
    ///
    /// They are queued only when the request asked to respond asynchronously,
    /// the batch reaches the configured threshold, and `target` is the live
    /// branch. A Snapshot branch always runs them inline: only the live
    /// branch's side-effect queue is drained, so a job queued in a Snapshot
    /// branch would never run.
    #[must_use]
    pub const fn side_effect_placement<'a>(&self, target: &'a ContextTarget, preference: AsyncPreference, batch_size: usize) -> SideEffectPlacement<'a> {
        resolve_side_effect_placement(target, self.batch_execution.async_side_effect_threshold, preference, batch_size)
    }
}

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration-scalars",
        "andromeda-context-commit",
        "andromeda-context-space",
        "andromeda-federation",
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
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-request",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-persistence-failure",
        "andromeda-spatial-index",
        "andromeda-subscription",
        "andromeda-telemetry",
        "andromeda-tenancy",
        "andromeda-unit-catalog",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}
