mod attrs;
pub mod batch;
/// Entity type and attribute discovery operations.
pub mod discovery;
/// Service-side `Value` extraction and persistence-params builders.
pub mod extract;
/// Provision (write) forwarding mode classification and multi-status helpers.
pub mod provision_forwarding;
/// Entity query and retrieval operations.
pub mod query;

use crate::{
    core::{batch_activity::BatchActivityTracker, error::AndromedaError},
    domain::{
        entity::{Entity, EntityPatch},
        expanded::removed_attribute_instances,
        notification::{ChangeEvent, TriggerReason},
        outbox::NewOutboxEntry,
        uri::Uri,
    },
    jsonld::{ContextSpec, JsonLdEngine},
    repository::{
        entity::{params::EntityCreateParams, repo::EntityRepository},
        pending_side_effects::payload::PersistedBatchSideEffectJob,
        temporal::repo::TemporalRepository,
        uow::UnitOfWorkFactory,
    },
    service::{
        entity::extract::{build_save_params, extract_geo_geometries},
        subscription_cache::SubscriptionCache,
        subscription_matching::matches_entity_filters,
        temporal_recorder::TemporalRecorder,
    },
};
use chrono::{DateTime, SecondsFormat, Utc};
use dashmap::DashMap;
use serde_json::{Map, Value};
use sqlx::postgres::PgPool;
use std::{
    collections::HashSet,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::sync::{Mutex, OwnedMutexGuard, broadcast};
use tracing::debug;
use urn_rs::Urn;

/// Map type used to coordinate writes by entity id.
///
/// See [`EntityService::acquire_id_locks`] for usage and lifetime.
type IdLockMap = DashMap<String, Arc<Mutex<()>>>;

/// Interval (in seconds) between sweeps of the per-id lock-coalescing map.
/// Sized to be roughly two orders of magnitude longer than a typical
/// chunk-write so the dominant cost is the writes themselves, not the
/// sweep.
const ID_LOCK_GC_INTERVAL_SECS: u64 = 60;

/// Configuration for outbox-based notification delivery (Phase 1: dual-write).
#[derive(Clone, Debug)]
pub struct OutboxWriteConfig {
    /// Shared subscription cache for matching entity mutations against subscriptions.
    pub subscription_cache: Arc<SubscriptionCache>,
}

/// Runtime tuning for batch entity execution paths.
#[derive(Clone, Debug)]
pub struct BatchExecutionConfig {
    /// Maximum number of concurrent entity preparations.
    pub concurrency: usize,
    /// Maximum number of chunks dispatched in parallel during large-batch processing.
    /// Sizes the fastpath preparation blocking-pool semaphore (CPU-bound work).
    pub chunk_concurrency: usize,
    /// Maximum number of write-batch chunks dispatched in parallel for
    /// create/upsert/update/merge/delete. Distinct from `chunk_concurrency`,
    /// which sizes fastpath preparation; write paths are DB-bound and prefer a
    /// lower ceiling to avoid row-lock contention on `entities` and
    /// `temporal_entity_attributes`.
    pub chunk_write_concurrency: usize,
    /// Maximum number of entities processed per chunk in large-batch mode.
    pub chunk_size: usize,
    /// Entity-count threshold that enables chunked large-batch processing.
    pub large_threshold: usize,
    /// Entity-count threshold that offloads preparation post-processing to blocking workers.
    pub prepare_blocking_threshold: usize,
    /// Minimum batch size for honouring `Prefer: respond-async`. When `None`,
    /// the async durable-queue path is honoured for any batch size whenever
    /// the client opts in via the header. When `Some(t)`, the async path is
    /// only taken for batches of size `>= t` (smaller batches stay inline
    /// regardless of the client preference).
    pub async_side_effect_threshold: Option<usize>,
}

/// Collected side-effects for a batch chunk: temporal history writes plus
/// NGSI-LD change-event emissions. Either run inline (sync path) or
/// persisted to `pending_side_effects` and drained by a background worker
/// (async path, opt-in via `Prefer: respond-async`).
///
/// Entity IDs and expanded payloads are `Arc`-wrapped so they can be
/// cheaply referenced both by the temporal writer (which reads them as
/// borrows) and by any future replay or fan-out path without deep clones.
#[derive(Debug)]
pub struct BatchSideEffectJob {
    pub temporal_entities: Vec<(Arc<str>, Arc<Value>)>,
    pub change_events: Vec<ChangeEvent>,
}

/// How batch side-effects (temporal writes + change events) should be
/// applied for a given request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchSideEffectMode {
    /// Run side-effects inline in the same request that triggered them.
    /// Strong read-your-write: the batch response implies the temporal
    /// history and any derived notifications have already been produced.
    Inline,
    /// Persist the side-effect job into `pending_side_effects` in the same
    /// transaction as the entity write; a background worker executes the
    /// job at-least-once. Opt-in via `Prefer: respond-async` plus the
    /// configured minimum-batch-size threshold.
    Enqueue,
}

/// Service layer for NGSI-LD entity CRUD operations.
pub struct EntityService {
    repo: EntityRepository,
    temporal_recorder: TemporalRecorder,
    uow: Arc<UnitOfWorkFactory>,
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
    /// Per-entity-id coalescing locks. Each chunk-write handler acquires the
    /// owned mutex guards for the ids it touches *before* asking the database
    /// pool for a connection. Concurrent batches with overlapping ids
    /// therefore serialise on broker-side mutexes (cheap) instead of
    /// `entities.id` row locks (which would otherwise hold a pool connection
    /// for the duration of the wait). See [`Self::acquire_id_locks`].
    id_locks: Arc<IdLockMap>,
}

impl EntityService {
    /// Create a new entity service backed by the given connection pool and shared temporal repository.
    pub fn new(
        pool: &PgPool,
        temporal_repo: Arc<TemporalRepository>,
        uow: Arc<UnitOfWorkFactory>,
        engine: JsonLdEngine,
        change_tx: Option<broadcast::Sender<ChangeEvent>>,
        batch_config: BatchExecutionConfig,
        batch_activity: BatchActivityTracker,
        outbox: Option<OutboxWriteConfig>,
    ) -> EntityService {
        let repo = EntityRepository::new(pool);
        let temporal_recorder = TemporalRecorder::new(temporal_repo);
        let id_locks: Arc<IdLockMap> = Arc::new(DashMap::new());
        spawn_id_lock_gc(Arc::downgrade(&id_locks));

        debug!("Entity service created!");

        EntityService {
            repo,
            temporal_recorder,
            uow,
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
    /// Callers must invoke this *before* asking the unit-of-work factory for
    /// a connection: the goal is to serialise overlapping batches on
    /// broker-side mutexes so the second caller never holds a pool slot while
    /// it waits.
    pub(crate) async fn acquire_id_locks(&self, ids: &[String]) -> Vec<OwnedMutexGuard<()>> {
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
        entity_types: Vec<String>,
        trigger: TriggerReason,
        changed_attributes: Vec<String>,
        previous: Option<Value>,
        current_payload: Option<Arc<Value>>,
    ) {
        let mut event = ChangeEvent::new(entity_id, entity_types, trigger)
            .with_changed_attributes(changed_attributes)
            .with_optional_previous(previous);
        if let Some(payload) = current_payload {
            event = event.with_current_payload(payload);
        }
        self.emit_change(event);
    }

    async fn fetch_entity_types(&self, id: &str) -> Vec<String> {
        match self.repo.get(id).await {
            Ok(Some(stored)) => extract_entity_types(&stored),
            _other => vec![],
        }
    }

    /// Emit a chunk's buffered change events inline.
    ///
    /// Only the batch **delete** path reaches this, and its job carries no
    /// temporal entities. Every path that does write temporal rows pairs them
    /// with the entity write inside a
    /// [`UnitOfWork`](crate::repository::uow::UnitOfWork), so there is no
    /// history left for this method to record.
    fn apply_inline_side_effects(&self, job: BatchSideEffectJob) {
        for event in job.change_events {
            self.emit_change(event);
        }
    }

    /// Persist a side-effect job into the durable queue inside the given
    /// unit of work. The caller must commit the UoW for the enqueue to take
    /// effect, guaranteeing crash-consistency with the entity write.
    pub(crate) async fn enqueue_batch_side_effects(
        &self,
        uow: &mut crate::repository::uow::UnitOfWork,
        job: &BatchSideEffectJob,
    ) -> Result<(), AndromedaError> {
        if job.temporal_entities.is_empty() && job.change_events.is_empty() {
            return Ok(());
        }
        let persisted: PersistedBatchSideEffectJob = job.into();
        uow.enqueue_pending_side_effects(&persisted).await?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // CRUD (write operations)
    // -------------------------------------------------------------------------

    /// Create a new NGSI-LD entity, returning its URN on success.
    pub async fn create(&self, entity: &Entity, context: &ContextSpec) -> Result<Urn, AndromedaError> {
        let id = entity.id.to_string();

        let mut entity_json = serde_json::to_value(entity).map_err(|err| AndromedaError::entity_validation(format!("Invalid entity payload: {}", err)))?;
        normalize_entity_attributes(&mut entity_json);
        let expanded = self.engine.expand(entity_json, context).await?;
        validate_attribute_envelopes(&expanded)?;

        let types = extract_entity_types(&expanded);
        let types_refs: Vec<&str> = types.iter().map(String::as_str).collect();
        let scopes = extract_scopes(&expanded);
        let scope_paths: Vec<String> = scopes.iter().map(|s| crate::query::sql::scope_to_ltree(s)).collect();
        let expires_at = crate::domain::expanded::extract_expires_at(&expanded);
        let (location, observation_space, operation_space) = extract_geo_geometries(&expanded);

        let outbox_entries = if self.outbox.is_some() {
            self.build_outbox_entries(
                &entity.id,
                &types,
                TriggerReason::EntityCreated,
                &extract_attribute_keys(&expanded),
                Some(&expanded),
                None,
            )
        } else {
            Vec::new()
        };

        let mut uow = self.uow.begin().await?;
        let urn = uow
            .create_entity(EntityCreateParams {
                id: &id,
                types: types_refs.as_ref(),
                scope_paths,
                payload: &expanded,
                location,
                observation_space,
                operation_space,
                expires_at,
            })
            .await?;
        uow.record_entity_attributes(&id, &expanded).await?;
        if !outbox_entries.is_empty() {
            uow.insert_outbox_batch(&outbox_entries).await?;
        }
        uow.commit().await?;

        let changed_attrs = extract_attribute_keys(&expanded);
        let payload_arc = Arc::new(expanded);
        self.emit_change(
            ChangeEvent::new(entity.id.clone(), types, TriggerReason::EntityCreated) // clone: owned Uri for event
                .with_changed_attributes(changed_attrs)
                .with_current_payload(payload_arc),
        );

        Ok(urn)
    }

    /// Merge a partial update into an existing entity (NGSI-LD 5.6.1).
    pub async fn merge(&self, id: &Urn, patch: &EntityPatch, context: &ContextSpec) -> Result<(), AndromedaError> {
        let entity_uri = Uri::from(id.clone()); // clone: owned Urn for Uri conversion; id is &Urn
        let id = id.to_string();

        let mut existing = self.repo.get(&id).await?.ok_or_else(|| AndromedaError::entity_not_found(&id))?;
        let attrs_before: HashSet<String> = extract_attribute_keys(&existing).into_iter().collect();
        let previous_attributes = self.change_tx.as_ref().map(|_| existing.clone()); // clone: snapshot before mutation for showChanges diff

        let mut patch_json = serde_json::to_value(patch).map_err(|err| AndromedaError::entity_validation(format!("Invalid patch payload: {}", err)))?;
        normalize_entity_attributes(&mut patch_json);
        let expanded = self.engine.expand(patch_json, context).await?;
        validate_attribute_envelopes(&expanded)?;
        crate::domain::expanded::merge_entity(&mut existing, &expanded).map_err(AndromedaError::internal)?;

        let types = self.fetch_entity_types(&id).await;
        let changed_attrs = extract_attribute_keys(&expanded);
        let attrs_after: HashSet<String> = extract_attribute_keys(&existing).into_iter().collect();
        let deleted_attrs: Vec<String> = attrs_before.difference(&attrs_after).cloned().collect(); // clone: owned strings from set difference

        let outbox_entries = if self.outbox.is_some() {
            let mut entries = Vec::new();
            for deleted_attr in &deleted_attrs {
                entries.extend(self.build_outbox_entries(
                    &entity_uri,
                    &types,
                    TriggerReason::AttributeDeleted,
                    std::slice::from_ref(deleted_attr),
                    Some(&existing),
                    previous_attributes.as_ref(),
                ));
            }
            entries.extend(self.build_outbox_entries(
                &entity_uri,
                &types,
                TriggerReason::EntityUpdated,
                &changed_attrs,
                Some(&existing),
                previous_attributes.as_ref(),
            ));
            entries
        } else {
            Vec::new()
        };

        let mut uow = self.uow.begin().await?;
        uow.save_entity(build_save_params(&id, &existing)).await?;
        uow.record_entity_attributes(&id, &expanded).await?;
        for deleted_attr in &deleted_attrs {
            uow.record_attribute_deletion(&id, deleted_attr, None).await?;
        }
        if !outbox_entries.is_empty() {
            uow.insert_outbox_batch(&outbox_entries).await?;
        }
        uow.commit().await?;

        let payload_arc = Arc::new(existing);
        for deleted_attr in &deleted_attrs {
            self.emit_attr_change(
                entity_uri.clone(), // clone: value needed by subsequent loop iterations and final emit
                types.clone(),      // clone: value needed by subsequent loop iterations and final emit
                TriggerReason::AttributeDeleted,
                vec![deleted_attr.clone()],  // clone: owned copy from borrowed iterator
                previous_attributes.clone(), // clone: value needed by subsequent loop iterations and final emit
                Some(Arc::clone(&payload_arc)),
            );
        }

        self.emit_attr_change(
            entity_uri,
            types,
            TriggerReason::EntityUpdated,
            changed_attrs,
            previous_attributes,
            Some(payload_arc),
        );

        Ok(())
    }

    /// Replace an entire entity with the given patch payload.
    pub async fn replace_entity(&self, id: &Urn, patch: &EntityPatch, context: &ContextSpec) -> Result<(), AndromedaError> {
        let entity_uri = Uri::from(id.clone()); // clone: owned Urn for Uri conversion; id is &Urn
        let id = id.to_string();

        // Existence is checked before expansion so a request against a missing
        // entity still reports 404 rather than a payload-validation error; the
        // authoritative read happens again inside the unit of work below.
        if self.repo.get(&id).await?.is_none() {
            return Err(AndromedaError::entity_not_found(id));
        }

        let mut patch_json = serde_json::to_value(patch).map_err(|err| AndromedaError::entity_validation(format!("Invalid patch payload: {}", err)))?;
        normalize_entity_attributes(&mut patch_json);
        let expanded = self.engine.expand(patch_json, context).await?;
        validate_attribute_envelopes(&expanded)?;

        let types = self.fetch_entity_types(&id).await;
        let changed_attrs = extract_attribute_keys(&expanded);

        let outbox_entries = if self.outbox.is_some() {
            self.build_outbox_entries(&entity_uri, &types, TriggerReason::EntityUpdated, &changed_attrs, Some(&expanded), None)
        } else {
            Vec::new()
        };

        // The prior payload is read inside the unit of work so the replacement
        // diff cannot be computed against a snapshot a concurrent writer has
        // already superseded. A full replacement drops every attribute the new
        // payload omits (NGSI-LD 5.6.3), and each of those needs a tombstone so
        // the temporal history and `attribute_current` follow the payload.
        let mut uow = self.uow.begin().await?;
        let Some(previous) = uow.get_entity(&id).await? else {
            return Err(AndromedaError::entity_not_found(id));
        };
        let removed = removed_attribute_instances(&previous, &expanded);
        uow.save_entity(build_save_params(&id, &expanded)).await?;
        uow.record_entity_attributes(&id, &expanded).await?;
        uow.record_removed_attributes(&id, &removed).await?;
        if !outbox_entries.is_empty() {
            uow.insert_outbox_batch(&outbox_entries).await?;
        }
        uow.commit().await?;

        let payload_arc = Arc::new(expanded);
        self.emit_change(
            ChangeEvent::new(entity_uri, types, TriggerReason::EntityUpdated)
                .with_changed_attributes(changed_attrs)
                .with_current_payload(payload_arc),
        );

        Ok(())
    }

    /// Delete an entity by its URN.
    pub async fn delete(&self, id: &Urn) -> Result<(), AndromedaError> {
        let entity_uri = Uri::from(id.clone()); // clone: owned Urn for Uri conversion; id is &Urn
        let id = id.to_string();

        let entity_snapshot = match self.repo.get(&id).await? {
            Some(expanded) => expanded,
            None => return Err(AndromedaError::entity_not_found(id)),
        };
        let entity_types = extract_entity_types(&entity_snapshot);

        // Capture sys_attrs before deletion so notifications can include them
        let sys_attrs_snapshot = match self.repo.get_sys_attrs(&id).await {
            Ok(attrs) => attrs,
            Err(e) => {
                tracing::warn!("Failed to fetch sys_attrs before delete for {}: {}", id, e);
                None
            }
        };

        let attr_keys = extract_attribute_keys(&entity_snapshot);

        let outbox_entries = if self.outbox.is_some() {
            let mut entries = Vec::new();
            if !attr_keys.is_empty() {
                entries.extend(self.build_outbox_entries(
                    &entity_uri,
                    &entity_types,
                    TriggerReason::AttributeDeleted,
                    &attr_keys,
                    Some(&entity_snapshot),
                    Some(&entity_snapshot),
                ));
            }
            entries.extend(self.build_outbox_entries(
                &entity_uri,
                &entity_types,
                TriggerReason::EntityDeleted,
                &[],
                Some(&entity_snapshot),
                Some(&entity_snapshot),
            ));
            entries
        } else {
            Vec::new()
        };

        let mut uow = self.uow.begin().await?;
        if uow.delete_entity(&id).await? == 0 {
            return Err(AndromedaError::entity_not_found(id));
        }
        if !outbox_entries.is_empty() {
            uow.insert_outbox_batch(&outbox_entries).await?;
        }
        uow.commit().await?;

        // Emit AttributeDeleted for each attribute so attributeDeleted subscriptions fire
        if !attr_keys.is_empty() {
            self.emit_change(
                ChangeEvent::new(entity_uri.clone(), entity_types.clone(), TriggerReason::AttributeDeleted) // clone: value also needed by subsequent EntityDeleted emit
                    .with_changed_attributes(attr_keys)
                    .with_snapshot(entity_snapshot.clone()) // clone: value also needed by subsequent EntityDeleted emit
                    .with_previous(entity_snapshot.clone()) // clone: value also needed by subsequent EntityDeleted emit
                    .with_sys_attrs(sys_attrs_snapshot.clone()), // clone: value also needed by subsequent EntityDeleted emit
            );
        }

        self.emit_change(
            ChangeEvent::new(entity_uri, entity_types, TriggerReason::EntityDeleted)
                .with_snapshot(entity_snapshot.clone()) // clone: separate ownership for snapshot and previous fields
                .with_previous(entity_snapshot)
                .with_sys_attrs(sys_attrs_snapshot),
        );

        Ok(())
    }

    /// Check whether an entity with the given URN exists.
    #[allow(dead_code)]
    pub async fn exists(&self, id: &Urn) -> Result<bool, AndromedaError> {
        let id = id.to_string();
        self.repo.exists(&id).await
    }

    /// Build outbox entries for matching subscriptions.
    ///
    /// Returns an empty vec if the subscription cache is not configured.
    /// Performs fast in-memory matching using the shared subscription cache,
    /// then applies entity-level filters (q, geoQ, scopeQ) for subscriptions
    /// that have them.
    fn build_outbox_entries(
        &self,
        entity_id: &Uri,
        entity_types: &[String],
        trigger: TriggerReason,
        changed_attrs: &[String],
        entity_snapshot: Option<&Value>,
        previous_snapshot: Option<&Value>,
    ) -> Vec<NewOutboxEntry> {
        let Some(outbox_config) = &self.outbox else {
            return Vec::new();
        };

        let event = ChangeEvent::new(entity_id.clone(), entity_types.to_vec(), trigger) // clone: owned Uri for ChangeEvent
            .with_changed_attributes(changed_attrs.to_vec());

        let matching = outbox_config.subscription_cache.find_matching(&event);

        // Apply entity-level filters for subscriptions that have q/geoQ/scopeQ
        let matching: Vec<_> = if let Some(data) = entity_snapshot {
            matching.into_iter().filter(|sub| matches_entity_filters(sub, data)).collect()
        } else {
            matching
        };

        matching
            .into_iter()
            .filter_map(|sub| {
                let subscription_id: Uri = match sub.id.parse() {
                    Ok(uri) => uri,
                    Err(e) => {
                        tracing::warn!(subscription_id = %sub.id, error = %e, "Skipping outbox entry: invalid subscription URI");
                        return None;
                    }
                };
                Some(NewOutboxEntry {
                    subscription_id,
                    entity_id: entity_id.clone(),                  // clone: owned Uri per entry
                    trigger,                                       // Copy
                    entity_snapshot: entity_snapshot.cloned(),     // clone: owned Value per entry
                    previous_snapshot: previous_snapshot.cloned(), // clone: owned Value per entry
                    changed_attrs: changed_attrs.to_vec(),
                })
            })
            .collect()
    }
}

/// Acquire mutex guards for `ids` against `map`, in deterministic sorted
/// order. Sorting eliminates lock-ordering deadlocks between concurrent
/// callers whose id sets overlap.
///
/// Callers receive owned guards so the locks remain held for the lifetime
/// of the returned `Vec`. Drop the vec to release every lock at once.
pub(crate) async fn acquire_id_locks_in(map: &IdLockMap, ids: &[String]) -> Vec<OwnedMutexGuard<()>> {
    let mut sorted: Vec<String> = ids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();

    let mut guards = Vec::with_capacity(sorted.len());
    for id in sorted {
        let lock = map.entry(id).or_insert_with(|| Arc::new(Mutex::new(()))).value().clone();
        guards.push(lock.lock_owned().await);
    }
    guards
}

/// Drop entries whose only `Arc` reference is the map itself: nothing else
/// is holding the mutex, so the entry can be reclaimed without losing
/// coordination.
///
/// `DashMap::retain` holds each bucket's lock while inspecting and removing
/// entries from that bucket. Concurrent calls to [`acquire_id_locks_in`]
/// for the same id must take the same bucket lock to insert or read the
/// `Arc<Mutex<()>>`, so they cannot observe a partially-removed state.
pub(crate) fn gc_id_locks(map: &IdLockMap) {
    map.retain(|_, lock| Arc::strong_count(lock) > 1);
}

/// Spawn a background task that periodically prunes unused entries from
/// `weak`. Holding a [`Weak`] instead of a strong [`Arc`] means the task
/// exits as soon as the owning [`EntityService`] is dropped — no manual
/// shutdown plumbing required.
fn spawn_id_lock_gc(weak: Weak<IdLockMap>) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(ID_LOCK_GC_INTERVAL_SECS));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await; // skip the immediate first tick.
        loop {
            ticker.tick().await;
            let Some(map) = weak.upgrade() else {
                break;
            };
            gc_id_locks(&map);
        }
    });
}

/// Pure routing logic for [`EntityService::effective_side_effect_mode`].
///
/// Split from the method so the decision table can be unit-tested without
/// constructing a full [`EntityService`]. See that method's doc comment for
/// the semantic contract.
pub(crate) const fn resolve_side_effect_mode(async_threshold: Option<usize>, client_requested_async: bool, batch_size: usize) -> BatchSideEffectMode {
    match (client_requested_async, async_threshold) {
        (true, None) => BatchSideEffectMode::Enqueue,
        (true, Some(threshold)) if batch_size >= threshold => BatchSideEffectMode::Enqueue,
        (true, Some(_)) | (false, None) | (false, Some(_)) => BatchSideEffectMode::Inline,
    }
}

// ---------------------------------------------------------------------------
// Shared helpers (used across submodules)
// ---------------------------------------------------------------------------

/// Re-export of type extraction for expanded JSON-LD entities.
pub(crate) use crate::domain::expanded::extract_types as extract_entity_types;

/// Extract scope values from an expanded JSON-LD entity.
pub(crate) fn extract_scopes(expanded: &Value) -> Vec<String> {
    let obj = match expanded {
        Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
        Value::Object(obj) => Some(obj),
        _other => None,
    };

    let Some(obj) = obj else {
        return Vec::new();
    };

    let Some(scope_value) = obj.get(vocab::expanded::properties::SCOPE.as_str()) else {
        return Vec::new();
    };

    match scope_value {
        Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_object().and_then(|obj| obj.get("@value")).and_then(|v| v.as_str()).map(String::from))
            .collect(),
        Value::String(s) => vec![s.clone()], // clone: owned String needed for return value from borrowed JSON
        _other => Vec::new(),
    }
}

fn extract_attribute_keys(expanded: &Value) -> Vec<String> {
    let obj = match expanded {
        Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
        Value::Object(obj) => Some(obj),
        _other => None,
    };

    obj.map(|o| o.keys().filter(|k| !k.starts_with('@')).cloned().collect()) // clone: owned keys from borrowed JSON map
        .unwrap_or_default()
}

fn unwrap_expanded_ref(value: &Value) -> &Value {
    match value {
        Value::Array(arr) if !arr.is_empty() => &arr[0],
        other => other,
    }
}

#[inline(always)]
pub(crate) fn normalize_entity_attributes(payload: &mut Value) {
    let Some(obj) = payload.as_object_mut() else {
        return;
    };

    for (key, value) in obj.iter_mut() {
        if matches!(
            key.as_str(),
            "id" | "type" | "scope" | "@context" | "@id" | "@type" | "expiresAt" | "createdAt" | "modifiedAt" | "deletedAt"
        ) {
            continue;
        }
        normalize_attribute_value(value);
    }
}

#[inline(always)]
fn normalize_attribute_value(value: &mut Value) {
    match value {
        Value::Object(obj) if is_explicit_attribute_object(obj) => {
            normalize_explicit_attribute_object(obj);
        }
        Value::Array(items) if is_explicit_attribute_array(items) => {
            for item in items {
                if let Value::Object(obj) = item {
                    normalize_explicit_attribute_object(obj);
                }
            }
        }
        ref _other => {
            let raw = std::mem::take(value);
            *value = wrap_raw_as_property(raw);
        }
    }
}

fn is_explicit_attribute_array(items: &[Value]) -> bool {
    !items.is_empty() && items.iter().all(|item| item.as_object().is_some_and(is_explicit_attribute_object))
}

fn is_explicit_attribute_object(obj: &Map<String, Value>) -> bool {
    obj.get("type").and_then(Value::as_str).is_some_and(is_known_attribute_type)
}

fn is_known_attribute_type(attr_type: &str) -> bool {
    attr_type.parse::<crate::domain::attribute_type::AttributeType>().is_ok()
}

fn normalize_explicit_attribute_object(obj: &mut Map<String, Value>) {
    let Some(attr_type_str) = obj.get("type").and_then(Value::as_str) else {
        return;
    };
    let reserved = match attr_type_str.parse::<crate::domain::attribute_type::AttributeType>() {
        Ok(t) => t.reserved_fields(),
        Err(_) => return,
    };

    for (key, value) in obj.iter_mut() {
        if key == "type" || reserved.contains(&key.as_str()) {
            continue;
        }
        normalize_attribute_value(value);
    }
}

fn wrap_raw_as_property(raw: Value) -> Value {
    let mut property = Map::new();
    property.insert("type".to_string(), Value::String("Property".to_string()));
    property.insert("value".to_string(), raw);
    Value::Object(property)
}

use crate::domain::vocab;

fn validate_attribute_envelopes(expanded: &Value) -> Result<(), AndromedaError> {
    let entities = match expanded {
        Value::Array(arr) => arr.as_slice(),
        _other => std::slice::from_ref(expanded),
    };

    for entity in entities {
        let Some(obj) = entity.as_object() else {
            continue;
        };
        for (key, value) in obj {
            if key.starts_with('@') {
                continue;
            }
            validate_attr_instances(key, value)?;
        }
    }
    Ok(())
}

fn validate_attr_instances(attr_name: &str, value: &Value) -> Result<(), AndromedaError> {
    let instances = match value {
        Value::Array(arr) => arr.as_slice(),
        _other => return Ok(()),
    };

    for instance in instances {
        let Some(obj) = instance.as_object() else {
            continue;
        };

        let types: Vec<&str> = obj
            .get("@type")
            .and_then(|t| t.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        let is_property = types.iter().any(|t| *t == vocab::expanded::classes::PROPERTY.as_str());
        let is_geo_property = types.iter().any(|t| *t == vocab::expanded::classes::GEO_PROPERTY.as_str());
        let is_relationship = types.iter().any(|t| *t == vocab::expanded::classes::RELATIONSHIP.as_str());

        if (is_property || is_geo_property) && !obj.contains_key(vocab::expanded::properties::VALUE.as_str()) {
            let short_type = if is_geo_property { "GeoProperty" } else { "Property" };
            return Err(AndromedaError::entity_validation(format!(
                "{} '{}' is missing required field 'value'",
                short_type, attr_name
            )));
        }

        if is_relationship && !obj.contains_key(vocab::expanded::properties::OBJECT.as_str()) {
            return Err(AndromedaError::entity_validation(format!(
                "Relationship '{}' is missing required field 'object'",
                attr_name
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared sysAttrs injection helpers (used by query + notification_worker)
// ---------------------------------------------------------------------------

/// Return a mutable reference to the top-level JSON object of an expanded entity.
pub(crate) fn expanded_entity_object_mut(entity: &mut Value) -> Option<&mut Map<String, Value>> {
    match entity {
        Value::Array(arr) => arr.first_mut().and_then(|v| v.as_object_mut()),
        Value::Object(obj) => Some(obj),
        _other => None,
    }
}

/// Inject system attributes (createdAt, modifiedAt, expiresAt) into an expanded entity.
pub(crate) fn inject_sys_attrs(entity: &mut Value, created_at: DateTime<Utc>, modified_at: DateTime<Utc>, expires_at: impl Into<Option<DateTime<Utc>>>) {
    let Some(obj) = expanded_entity_object_mut(entity) else {
        return;
    };
    let default_context_prefix = format!("{}default-context/", vocab::prefix::NGSI_LD.as_str());

    let created_at_value = serde_json::json!([{
        "@type": vocab::expanded::classes::DATE_TIME.as_str(),
        "@value": created_at.to_rfc3339_opts(SecondsFormat::Millis, true)
    }]);
    let modified_at_value = serde_json::json!([{
        "@type": vocab::expanded::classes::DATE_TIME.as_str(),
        "@value": modified_at.to_rfc3339_opts(SecondsFormat::Millis, true)
    }]);

    obj.entry(vocab::expanded::properties::CREATED_AT.as_str().to_string())
        .or_insert(created_at_value.clone()); // clone: value also passed to inject_attr_sys_attrs below
    obj.entry(vocab::expanded::properties::MODIFIED_AT.as_str().to_string())
        .or_insert(modified_at_value.clone()); // clone: value also passed to inject_attr_sys_attrs below

    if let Some(expires_at) = expires_at.into() {
        let expires_at_value = serde_json::json!([{
            "@type": vocab::expanded::classes::DATE_TIME.as_str(),
            "@value": expires_at.to_rfc3339_opts(SecondsFormat::Millis, true)
        }]);
        obj.entry(vocab::expanded::properties::EXPIRES_AT.as_str().to_string())
            .or_insert(expires_at_value);
    }

    inject_attr_sys_attrs(obj, &created_at_value, &modified_at_value, &default_context_prefix);
}

/// Inject system attributes into each attribute instance within an entity object.
pub(crate) fn inject_attr_sys_attrs(obj: &mut Map<String, Value>, created_at_value: &Value, modified_at_value: &Value, default_context_prefix: &str) {
    let attr_keys: Vec<String> = obj
        .keys()
        .filter(|k| !k.starts_with('@') && (!k.starts_with(vocab::prefix::NGSI_LD.as_str()) || k.starts_with(default_context_prefix)))
        .cloned() // clone: owned keys needed to avoid borrowing obj during mutation below
        .collect();

    for key in attr_keys {
        let Some(arr) = obj.get_mut(&key).and_then(|v| v.as_array_mut()) else {
            continue;
        };
        for instance in arr.iter_mut() {
            let Some(inst_obj) = instance.as_object_mut() else {
                continue;
            };
            inst_obj
                .entry(vocab::expanded::properties::CREATED_AT.as_str().to_string())
                .or_insert(created_at_value.clone()); // clone: or_insert requires owned value, reused across loop iterations
            inst_obj
                .entry(vocab::expanded::properties::MODIFIED_AT.as_str().to_string())
                .or_insert(modified_at_value.clone()); // clone: or_insert requires owned value, reused across loop iterations
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::{BatchSideEffectMode, IdLockMap, acquire_id_locks_in, gc_id_locks, normalize_entity_attributes, resolve_side_effect_mode};
    use std::sync::Arc;

    #[test]
    fn prefer_respond_async_with_none_threshold_routes_to_enqueue() {
        assert_eq!(resolve_side_effect_mode(None, true, 1), BatchSideEffectMode::Enqueue);
    }

    #[test]
    fn prefer_respond_async_with_threshold_below_batch_size_routes_to_inline() {
        assert_eq!(resolve_side_effect_mode(Some(100), true, 10), BatchSideEffectMode::Inline);
    }

    #[test]
    fn prefer_respond_async_at_or_above_threshold_routes_to_enqueue() {
        assert_eq!(resolve_side_effect_mode(Some(100), true, 100), BatchSideEffectMode::Enqueue);
        assert_eq!(resolve_side_effect_mode(Some(100), true, 512), BatchSideEffectMode::Enqueue);
    }

    #[test]
    fn prefer_respond_sync_forces_inline_with_threshold_zero() {
        assert_eq!(resolve_side_effect_mode(Some(0), false, 1000), BatchSideEffectMode::Inline);
    }

    #[test]
    fn no_async_opt_in_routes_to_inline_regardless_of_threshold() {
        assert_eq!(resolve_side_effect_mode(None, false, 1000), BatchSideEffectMode::Inline);
        assert_eq!(resolve_side_effect_mode(Some(1), false, 1000), BatchSideEffectMode::Inline);
    }

    #[tokio::test]
    async fn id_lock_gc_reclaims_unused_entries_after_guards_drop() {
        let map: Arc<IdLockMap> = Arc::new(IdLockMap::new());
        let ids = vec!["urn:ngsi-ld:A".to_string(), "urn:ngsi-ld:B".to_string(), "urn:ngsi-ld:C".to_string()];

        let guards = acquire_id_locks_in(&map, &ids).await;
        pretty_assertions::assert_eq!(map.len(), 3);

        // While guards are alive, GC must keep every entry — strong_count > 1.
        gc_id_locks(&map);
        pretty_assertions::assert_eq!(map.len(), 3);

        drop(guards);
        // Once the only outstanding `Arc` is the map's own, GC reclaims the entry.
        gc_id_locks(&map);
        pretty_assertions::assert_eq!(map.len(), 0);
    }

    #[tokio::test]
    async fn id_lock_acquire_dedupes_and_orders_ids() {
        let map: Arc<IdLockMap> = Arc::new(IdLockMap::new());
        // Duplicates must collapse to a single entry; sorted ordering eliminates
        // deadlocks between concurrent callers with overlapping id sets.
        let ids = vec!["b".to_string(), "a".to_string(), "b".to_string(), "c".to_string(), "a".to_string()];
        let guards = acquire_id_locks_in(&map, &ids).await;
        pretty_assertions::assert_eq!(guards.len(), 3);
        pretty_assertions::assert_eq!(map.len(), 3);
    }

    #[test]
    fn normalize_entity_attributes_wraps_concise_values_as_property() {
        let mut payload = serde_json::json!({
            "id": "urn:ngsi-ld:Testing:1",
            "type": "Testing",
            "A1": "value of A1",
            "A2": { "nested": true }
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["A1"]["type"], "Property");
        pretty_assertions::assert_eq!(payload["A1"]["value"], "value of A1");
        pretty_assertions::assert_eq!(payload["A2"]["type"], "Property");
        pretty_assertions::assert_eq!(payload["A2"]["value"], serde_json::json!({ "nested": true }));
    }

    #[test]
    fn normalize_entity_attributes_keeps_typed_attributes() {
        let mut payload = serde_json::json!({
            "id": "urn:ngsi-ld:Testing:2",
            "type": "Testing",
            "name": {
                "type": "Property",
                "value": "Andromeda"
            }
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["name"]["type"], "Property");
        pretty_assertions::assert_eq!(payload["name"]["value"], "Andromeda");
    }

    #[test]
    fn normalize_entity_attributes_wraps_concise_attr_entries() {
        let mut payload = serde_json::json!({
            "A1": 1,
            "A2": false
        });

        normalize_entity_attributes(&mut payload);

        pretty_assertions::assert_eq!(payload["A1"]["type"], "Property");
        pretty_assertions::assert_eq!(payload["A1"]["value"], 1);
        pretty_assertions::assert_eq!(payload["A2"]["type"], "Property");
        pretty_assertions::assert_eq!(payload["A2"]["value"], false);
    }
}
