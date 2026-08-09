//! The entity policy: clause 5.6 write operations and clause 5.7 discovery,
//! expressed against ports rather than against a database.

/// Checking attribute instance envelopes (clause 5.2.5).
pub mod attribute_envelope;
/// Attribute-level writes (clauses 5.6.2 to 5.6.5 and 5.6.19).
pub mod attrs;
pub mod batch;
/// Cache of compiled entity queries, owned by the query service.
pub mod compiled_cache;
/// Entity type and attribute discovery operations.
pub mod discovery;
/// Reading members out of an expanded JSON-LD entity.
pub mod expanded_member;
/// Coalescing concurrent writers by entity identifier.
pub mod id_lock;
/// Whole-entity mutations (clauses 5.6.1 to 5.6.6).
pub mod mutation;
/// Turning one mutation into the outbox entries its subscribers need.
pub mod outbox_fanout;
/// Provision (write) forwarding mode classification and multi-status helpers.
pub mod provision_forwarding;
/// Purging attributes from a set of entities.
pub mod purge;
/// Entity query and retrieval operations.
pub mod query;
/// Turning a read plus a computation into an optimistic write.
pub mod read_modify_write;
/// The predicates a store declined to apply.
pub mod residual_filter;
/// Work a batch write produces but need not finish before answering.
pub mod side_effect;
/// The durable side-effect queue port.
pub mod side_effect_queue;
/// The entity store port, declared by the policy that calls it.
pub mod store;
/// Injecting the clause 4.8 system attributes into an expanded entity.
pub mod sys_attrs;
use crate::{
    batch_activity::BatchActivityTracker,
    service::{
        entity::{
            batch::execution::BatchExecutionConfig,
            id_lock::{IdLockMap, acquire_id_locks_in, spawn_id_lock_gc},
            side_effect::{BatchSideEffectMode, resolve_side_effect_mode},
            store::EntityStore,
        },
        subscription_cache::SubscriptionCache,
    },
};
use andromeda_context_commit::{batch_side_effect_job::BatchSideEffectJob, change_event::ChangeEvent, committer::ContextCommitter};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::{ChangeOrigin, NotificationTrigger};
use andromeda_postgresql_temporal_store::recorder::TemporalRecorder;
use chrono::Utc;
use dashmap::DashMap;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{OwnedMutexGuard, broadcast};
use tracing::debug;

/// Configuration for outbox-based notification delivery.
#[derive(Clone, Debug)]
pub struct OutboxWriteConfig {
    /// Shared subscription cache for matching entity mutations against subscriptions.
    pub subscription_cache: Arc<SubscriptionCache>,
}

/// Service layer for NGSI-LD entity CRUD operations.
pub struct EntityService {
    repo: Arc<dyn EntityStore>,
    /// Reads the temporal catalogue when clause 5.6.5 needs to know which
    /// `datasetId`s an attribute has ever had. Step 11 of
    /// `docs/workspace-topology.md` replaces this with a temporal port.
    temporal_recorder: Arc<TemporalRecorder>,
    writes: Arc<dyn ContextCommitter>,
    engine: JsonLdEngine,
    change_tx: Option<broadcast::Sender<ChangeEvent>>,
    batch_activity: BatchActivityTracker,
    batch_chunk_size: usize,
    batch_chunk_concurrency: usize,
    batch_chunk_write_concurrency: usize,
    batch_concurrency: usize,
    batch_large_threshold: usize,
    batch_prepare_blocking_threshold: usize,
    async_side_effect_threshold: Option<usize>,
    /// Outbox write configuration, present when outbox is enabled.
    outbox: Option<OutboxWriteConfig>,
    /// Per-entity-id coalescing locks. See [`id_lock`].
    id_locks: Arc<IdLockMap>,
}

impl EntityService {
    /// Create a new entity service over the given store and write coordinator.
    pub fn new(
        repo: Arc<dyn EntityStore>,
        temporal_recorder: Arc<TemporalRecorder>,
        writes: Arc<dyn ContextCommitter>,
        engine: JsonLdEngine,
        change_tx: Option<broadcast::Sender<ChangeEvent>>,
        batch_config: BatchExecutionConfig,
        batch_activity: BatchActivityTracker,
        outbox: Option<OutboxWriteConfig>,
    ) -> EntityService {
        let id_locks: Arc<IdLockMap> = Arc::new(DashMap::new());
        spawn_id_lock_gc(Arc::downgrade(&id_locks));

        debug!("Entity service created!");

        EntityService {
            repo,
            temporal_recorder,
            writes,
            engine,
            change_tx,
            batch_activity,
            batch_chunk_size: batch_config.chunk_size.max(1),
            batch_chunk_concurrency: batch_config.chunk_concurrency.max(1),
            batch_chunk_write_concurrency: batch_config.chunk_write_concurrency.max(1),
            batch_concurrency: batch_config.concurrency.max(1),
            batch_large_threshold: batch_config.large_threshold.max(1),
            batch_prepare_blocking_threshold: batch_config.prepare_blocking_threshold.max(1),
            async_side_effect_threshold: batch_config.async_side_effect_threshold,
            outbox,
            id_locks,
        }
    }

    /// Acquire owned mutex guards for every distinct id in `ids`, in sorted
    /// order so concurrent callers cannot deadlock. Locks are released when
    /// the returned `Vec` is dropped.
    ///
    /// Callers must invoke this *before* asking the coordinator to commit: the
    /// goal is to serialise overlapping batches on broker-side mutexes so the
    /// second caller never holds a pool slot while it waits.
    pub(crate) async fn acquire_id_locks(&self, ids: &[Uri]) -> Vec<OwnedMutexGuard<()>> {
        acquire_id_locks_in(&self.id_locks, ids).await
    }

    /// Decide which side-effect path to take based on the client's
    /// `Prefer: respond-async` hint and the configured minimum-batch
    /// threshold. When the client opts in and no threshold is configured the
    /// async path is taken for any batch size; with a threshold, only batches
    /// at or above it are enqueued. Requests that do not opt in always run
    /// inline. See NGSI-LD 6.3.5 for the server's discretion over `Prefer`
    /// token handling.
    pub const fn effective_side_effect_mode(&self, client_requested_async: bool, batch_size: usize) -> BatchSideEffectMode {
        resolve_side_effect_mode(self.async_side_effect_threshold, client_requested_async, batch_size)
    }

    fn emit_change(&self, event: ChangeEvent) {
        if let Some(ref tx) = self.change_tx {
            let _ = tx.send(event);
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

    /// Emit a chunk's buffered change events inline.
    fn apply_inline_side_effects(&self, job: BatchSideEffectJob) {
        for event in job.change_events {
            self.emit_change(event);
        }
    }
}
