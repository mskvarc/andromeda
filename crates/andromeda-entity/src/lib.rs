//! Entity policy for ETSI GS CIM 009 clauses 5.6 and 5.7.

#![deny(missing_docs)]

/// Checking attribute instance envelopes (clause 5.2.5).
pub mod attribute_envelope;
/// Aggregated attribute discovery values.
pub mod attribute_summary;
/// Attribute-level writes (clauses 5.6.2 to 5.6.5 and 5.6.19).
pub mod attrs;
/// Batch entity operations and execution policy.
pub mod batch;
/// Tracking recent batch activity for coordinated background work.
pub mod batch_activity;
/// Publication boundary for committed entity changes.
pub mod change_sink;
/// Cache of compiled entity queries, owned by the query service.
pub mod compiled_cache;
/// Entity execution and background-operation configuration.
pub mod config;
/// Entity type and attribute discovery operations.
pub mod discovery;
/// Versioned entity snapshots used by optimistic writes.
pub mod entity_snapshot;
/// Entity policy failures.
pub mod error;
/// Reading members out of an expanded JSON-LD entity.
pub mod expanded_member;
/// Applying entity-level and attribute-instance expiry policy.
pub mod expiry;

pub mod geo_property_geometry;
/// Coalescing concurrent writers by entity identifier.
pub mod id_lock;
/// Whole-entity mutations (clauses 5.6.1 to 5.6.6).
pub mod mutation;
/// Turning one mutation into the outbox entries its subscribers need.
pub mod outbox_fanout;
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
/// Evaluating predicates a store declined to apply.
pub mod residual_filter;
/// One named query predicate a store may decline.
pub mod residual_predicate;
/// Work a batch write produces but need not finish before answering.
pub mod side_effect;
pub mod side_effect_drain;
/// The durable side-effect queue port.
pub mod side_effect_queue;
/// Entity spatial-index synchronization configuration.
pub mod spatial_config;
pub mod spatial_reconciliation;
pub mod spatial_synchronization;
/// Query capabilities of one entity store.
pub mod storage_profile;
/// Injecting clause 4.8 system attributes into an expanded entity.
pub mod sys_attrs;

use crate::{
    batch::execution::BatchExecutionConfig,
    batch_activity::BatchActivityTracker,
    change_sink::EntityChangeSink,
    id_lock::{IdLockMap, acquire_id_locks_in},
    port::{attribute_history::AttributeDatasetHistory, blocking_executor::BlockingExecutor, reader::EntityReader},
    side_effect::{AsyncPreference, BatchSideEffectMode, resolve_side_effect_mode},
};
use andromeda_context_commit::{batch_side_effect_job::BatchSideEffectJob, change_event::ChangeEvent, committer::ContextCommitter};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::{ChangeOrigin, NotificationTrigger};
use andromeda_subscription::cache::SubscriptionCache;
use async_lock::MutexGuardArc;
use chrono::Utc;
use dashmap::DashMap;
use serde_json::Value;
use std::sync::Arc;

/// Configuration for outbox-based notification delivery.
#[derive(Clone, Debug)]
pub struct OutboxWriteConfig {
    /// Shared subscription cache for matching entity mutations against subscriptions.
    pub subscription_cache: Arc<SubscriptionCache>,
}

/// Policy for NGSI-LD entity CRUD and batch operations.
pub struct EntityService {
    reader: Arc<dyn EntityReader>,
    blocking_executor: Arc<dyn BlockingExecutor>,
    attribute_history: Arc<dyn AttributeDatasetHistory>,
    writes: Arc<dyn ContextCommitter>,
    engine: JsonLdEngine,
    change_sink: Option<Arc<dyn EntityChangeSink>>,
    batch_activity: BatchActivityTracker,
    batch_execution: BatchExecutionConfig,
    outbox: Option<OutboxWriteConfig>,
    id_locks: Arc<IdLockMap>,
}

impl EntityService {
    /// Construct entity policy over caller-owned ports and runtime bindings.
    pub fn new(
        reader: Arc<dyn EntityReader>,
        blocking_executor: Arc<dyn BlockingExecutor>,
        attribute_history: Arc<dyn AttributeDatasetHistory>,
        writes: Arc<dyn ContextCommitter>,
        engine: JsonLdEngine,
        change_sink: Option<Arc<dyn EntityChangeSink>>,
        batch_config: BatchExecutionConfig,
        batch_activity: BatchActivityTracker,
        outbox: Option<OutboxWriteConfig>,
    ) -> EntityService {
        let id_locks: Arc<IdLockMap> = Arc::new(DashMap::new());
        let batch_execution = BatchExecutionConfig {
            concurrency: batch_config.concurrency.max(1),
            chunk_concurrency: batch_config.chunk_concurrency.max(1),
            chunk_write_concurrency: batch_config.chunk_write_concurrency.max(1),
            chunk_size: batch_config.chunk_size.max(1),
            large_threshold: batch_config.large_threshold.max(1),
            prepare_blocking_threshold: batch_config.prepare_blocking_threshold.max(1),
            async_side_effect_threshold: batch_config.async_side_effect_threshold,
        };

        EntityService {
            reader,
            blocking_executor,
            attribute_history,
            writes,
            engine,
            change_sink,
            batch_activity,
            batch_execution,
            outbox,
            id_locks,
        }
    }

    /// Acquire owned locks for every distinct identifier in sorted order.
    pub(crate) async fn acquire_id_locks(&self, ids: &[Uri]) -> Vec<MutexGuardArc<()>> {
        acquire_id_locks_in(&self.id_locks, ids).await
    }

    /// Select inline or durable queued side effects for one batch.
    pub const fn effective_side_effect_mode(&self, preference: AsyncPreference, batch_size: usize) -> BatchSideEffectMode {
        resolve_side_effect_mode(self.batch_execution.async_side_effect_threshold, preference, batch_size)
    }

    fn emit_change(&self, event: ChangeEvent) {
        if let Some(ref sink) = self.change_sink {
            sink.publish(event);
        }
    }

    fn emit_attr_change(
        &self,
        entity_id: Uri,
        entity_types: Vec<EntityTypeName>,
        trigger: NotificationTrigger,
        changed_attributes: Vec<AttributeName>,
        previous: Option<Value>,
        current_payload: Option<Arc<Value>>,
    ) {
        self.emit_change(
            ChangeEvent::builder()
                .entity_id(entity_id)
                .entity_types(entity_types)
                .trigger_reason(trigger)
                .occurred_at(Utc::now())
                .origin(ChangeOrigin::Local)
                .changed_attributes(changed_attributes)
                .previous_attributes(previous)
                .current_payload(current_payload)
                .build(),
        );
    }

    fn apply_inline_side_effects(&self, job: BatchSideEffectJob) {
        job.change_events.into_iter().for_each(|event| self.emit_change(event));
    }
}

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-commit",
        "andromeda-federation",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-request",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-spatial-index",
        "andromeda-subscription",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["confique"],
}
