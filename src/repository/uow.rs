//! Unit-of-work abstraction wrapping a single Postgres transaction.
//!
//! Owned by the persistence layer so service code can compose entity,
//! temporal, outbox, and pending-side-effect writes into one commit
//! without importing `sqlx`. Each method delegates to an existing
//! `*_in_tx` repository function against the held transaction.

use crate::{
    core::{config::BulkWriteMode, error::AndromedaError},
    repository::{
        entity::{
            copy::create_batch_via_copy_in_tx,
            params::{EntityCreateParams, EntitySaveParams, PreparedEntityColumns},
            repo::EntityRepository,
            rows::{DeletedEntity, PurgedEntity},
        },
        outbox::repo::OutboxRepository,
        pending_side_effects::{payload::PersistedBatchSideEffectJob, repo::PendingSideEffectsRepository},
        temporal::{params::HistoryBatchEntry, repo::TemporalRepository},
    },
    service::temporal_recorder::TemporalRecorder,
    worker::attribute_current_materializer::AttributeCurrentQueue,
};
use andromeda_ngsi_ld_model::{expanded::instance_key::AttributeInstanceKey, outbox::NewOutboxEntry};
use iri_rs::IriBuf;
use serde_json::Value;
use sqlx::{Postgres, Transaction, postgres::PgPool};
use std::{collections::HashMap, sync::Arc};
use urn_rs::Urn;
use uuid::Uuid;

/// Tenant-scoped factory for [`UnitOfWork`] instances.
///
/// Holds the per-tenant connection pool, the shared
/// [`TemporalRecorder`], the bulk-write mode used to dispatch entity
/// batch inserts, and the post-commit `attribute_current` materializer
/// channel sender. Constructed once per tenant in `build_tenant_state`
/// and shared via `Arc`.
#[derive(Debug)]
pub struct UnitOfWorkFactory {
    pool: PgPool,
    temporal_recorder: Arc<TemporalRecorder>,
    bulk_write_mode: BulkWriteMode,
    attribute_current_queue: AttributeCurrentQueue,
}

impl UnitOfWorkFactory {
    /// Create a factory backed by the given pool and recorder.
    pub fn new(
        pool: &PgPool,
        temporal_repo: Arc<TemporalRepository>,
        bulk_write_mode: BulkWriteMode,
        attribute_current_queue: AttributeCurrentQueue,
    ) -> UnitOfWorkFactory {
        UnitOfWorkFactory {
            pool: pool.clone(), // clone: Arc ref-count bump
            temporal_recorder: Arc::new(TemporalRecorder::new(temporal_repo)),
            bulk_write_mode,
            attribute_current_queue,
        }
    }

    /// Open a new unit-of-work transaction.
    #[tracing::instrument(skip_all)]
    pub async fn begin(&self) -> Result<UnitOfWork, AndromedaError> {
        let tx = self.pool.begin().await?;
        Ok(UnitOfWork {
            tx: Some(tx),
            temporal_recorder: Arc::clone(&self.temporal_recorder),
            bulk_write_mode: self.bulk_write_mode,
            attribute_current_queue: self.attribute_current_queue.clone(), // clone: per-UoW handle (mpsc Sender clone is cheap)
            pending_attribute_current: Vec::new(),
        })
    }
}

/// Active unit-of-work bound to a single Postgres transaction.
///
/// On drop without an explicit `commit()`, the held transaction is
/// dropped and sqlx rolls back. A `tracing::debug!` event is emitted
/// for observability; rollback errors themselves are swallowed by
/// sqlx. The `tx` field is `Option`-wrapped so `commit()` can move
/// the transaction out without conflicting with `Drop`.
///
/// `pending_attribute_current` accumulates history rows that will be
/// projected into `attribute_current` after `commit()` succeeds. They
/// are dispatched via the materializer channel exactly once, after
/// commit; if the unit-of-work rolls back, the entries are dropped.
pub struct UnitOfWork {
    tx: Option<Transaction<'static, Postgres>>,
    temporal_recorder: Arc<TemporalRecorder>,
    bulk_write_mode: BulkWriteMode,
    attribute_current_queue: AttributeCurrentQueue,
    pending_attribute_current: Vec<HistoryBatchEntry>,
}

impl UnitOfWork {
    fn tx_mut(&mut self) -> Result<&mut Transaction<'static, Postgres>, AndromedaError> {
        self.tx
            .as_mut()
            .ok_or_else(|| AndromedaError::internal("UnitOfWork used after commit".to_string()))
    }

    // ---------------------------------------------------------------------
    // Entity writes
    // ---------------------------------------------------------------------

    /// Read a single entity payload inside this unit of work.
    pub async fn get_entity(&mut self, id: &str) -> Result<Option<Value>, AndromedaError> {
        EntityRepository::get_in_tx(self.tx_mut()?, id).await
    }

    /// Read several entity payloads inside this unit of work.
    pub async fn get_entities(&mut self, ids: &[String]) -> Result<HashMap<String, Value>, AndromedaError> {
        EntityRepository::get_batch_in_tx(self.tx_mut()?, ids).await
    }

    /// Insert a single entity row.
    pub async fn create_entity(&mut self, params: EntityCreateParams<'_>) -> Result<Urn, AndromedaError> {
        EntityRepository::create_in_tx(self.tx_mut()?, params).await
    }

    /// Insert the `entities` row a temporal write depends on, leaving any
    /// existing row untouched.
    ///
    /// See [`EntityRepository::ensure_placeholder_in_tx`] for why this is not
    /// [`Self::create_entity`] with a conflict clause.
    pub async fn ensure_entity_placeholder(&mut self, params: EntityCreateParams<'_>) -> Result<(), AndromedaError> {
        EntityRepository::ensure_placeholder_in_tx(self.tx_mut()?, params).await
    }

    /// Update a single entity row, returning rows affected.
    pub async fn save_entity(&mut self, params: EntitySaveParams<'_>) -> Result<u64, AndromedaError> {
        EntityRepository::save_in_tx(self.tx_mut()?, params).await
    }

    /// Delete a single entity row, returning rows affected.
    pub async fn delete_entity(&mut self, id: &str) -> Result<u64, AndromedaError> {
        EntityRepository::delete_in_tx(self.tx_mut()?, id).await
    }

    /// Bulk-insert entities, dispatching on the configured bulk-write mode.
    pub async fn create_entities_batch(&mut self, columns: &PreparedEntityColumns) -> Result<Vec<String>, AndromedaError> {
        match self.bulk_write_mode {
            BulkWriteMode::Unnest => EntityRepository::create_batch_in_tx(self.tx_mut()?, columns).await,
            BulkWriteMode::CopyStaging => create_batch_via_copy_in_tx(self.tx_mut()?, columns).await,
        }
    }

    /// Bulk-update entities by id, returning the ids of rows that were
    /// actually updated.
    pub async fn save_entities_batch(&mut self, columns: &PreparedEntityColumns) -> Result<Vec<String>, AndromedaError> {
        EntityRepository::save_batch_in_tx(self.tx_mut()?, columns).await
    }

    /// Bulk-upsert entities, returning per-row `(id, was_created)` pairs.
    pub async fn upsert_entities_batch(&mut self, columns: &PreparedEntityColumns) -> Result<Vec<(String, bool)>, AndromedaError> {
        EntityRepository::upsert_batch_in_tx(self.tx_mut()?, columns).await
    }

    /// Bulk-delete entities by id, returning the deleted-row snapshots.
    pub async fn delete_entities_batch(&mut self, ids: &[String]) -> Result<Vec<DeletedEntity>, AndromedaError> {
        EntityRepository::delete_batch_in_tx(self.tx_mut()?, ids).await
    }

    /// Remove past-due attribute instances from every affected entity payload,
    /// returning the before/after payloads so the caller can tombstone them.
    pub async fn purge_expired_attributes(&mut self) -> Result<Vec<PurgedEntity>, AndromedaError> {
        EntityRepository::purge_expired_attributes_in_tx(self.tx_mut()?).await
    }

    // ---------------------------------------------------------------------
    // Temporal writes
    // ---------------------------------------------------------------------

    /// Record all attribute observations from one expanded entity.
    pub async fn record_entity_attributes(&mut self, id: &str, expanded: &Value) -> Result<(), AndromedaError> {
        let recorder = Arc::clone(&self.temporal_recorder); // clone: Arc ref-count bump to allow tx_mut borrow concurrently
        let history = recorder.record_entity_attributes_in_tx(self.tx_mut()?, id, expanded).await?;
        self.pending_attribute_current.extend(history);
        Ok(())
    }

    /// Record tombstones for every attribute instance a replacement dropped.
    ///
    /// Pair with [`andromeda_ngsi_ld_model::expanded::instance_key::removed_attribute_instances`] on
    /// any path that overwrites `entities.payload` wholesale: without this the
    /// temporal history never learns the attribute went away and
    /// `attribute_current` keeps serving its last value to `q` filters.
    pub async fn record_removed_attributes(&mut self, id: &str, removed: &[AttributeInstanceKey]) -> Result<(), AndromedaError> {
        for key in removed {
            self.record_attribute_deletion(id, key.attr.as_str(), key.dataset_id.as_ref().map(IriBuf::as_str))
                .await?;
        }
        Ok(())
    }

    /// Record a tombstone for a deleted attribute.
    pub async fn record_attribute_deletion(&mut self, id: &str, attr_iri: &str, dataset_id: Option<&str>) -> Result<(), AndromedaError> {
        let recorder = Arc::clone(&self.temporal_recorder); // clone: Arc ref-count bump to allow tx_mut borrow concurrently
        let history = recorder.record_attribute_deletion_in_tx(self.tx_mut()?, id, attr_iri, dataset_id).await?;
        self.pending_attribute_current.extend(history);
        Ok(())
    }

    /// Record a batch of expanded entity observations in one call.
    pub async fn record_entities_batch(&mut self, entries: &[(Arc<str>, Arc<Value>)]) -> Result<(), AndromedaError> {
        let recorder = Arc::clone(&self.temporal_recorder); // clone: Arc ref-count bump to allow tx_mut borrow concurrently
        let tx = self.tx_mut()?;
        let history = recorder.record_entities_batch_in_tx(tx, entries).await?;
        self.pending_attribute_current.extend(history);
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Side-effect queue
    // ---------------------------------------------------------------------

    /// Append a side-effect job to the durable queue.
    pub async fn enqueue_pending_side_effects(&mut self, payload: &PersistedBatchSideEffectJob) -> Result<Uuid, AndromedaError> {
        PendingSideEffectsRepository::enqueue_in_tx(self.tx_mut()?, payload).await
    }

    // ---------------------------------------------------------------------
    // Outbox
    // ---------------------------------------------------------------------

    /// Insert a batch of notification-outbox entries.
    pub async fn insert_outbox_batch(&mut self, entries: &[NewOutboxEntry]) -> Result<(), AndromedaError> {
        OutboxRepository::insert_batch_in_tx(self.tx_mut()?, entries).await
    }

    // ---------------------------------------------------------------------
    // Lifecycle
    // ---------------------------------------------------------------------

    /// Commit the underlying transaction. Subsequent operations on this
    /// instance will return an error.
    ///
    /// Once the database commit succeeds, any queued
    /// `attribute_current` updates are dispatched on the materializer
    /// queue. The queue is bounded, so a materializer that has fallen
    /// behind delays this call — the transaction is already committed and
    /// released at that point, so the wait costs no database resources. A
    /// closed channel (worker exited) is intentionally swallowed:
    /// `attribute_history` is the canonical source, and the startup
    /// reconcile will refill the projection.
    #[tracing::instrument(skip_all)]
    pub async fn commit(mut self) -> Result<(), AndromedaError> {
        let tx = self
            .tx
            .take()
            .ok_or_else(|| AndromedaError::internal("UnitOfWork commit called twice".to_string()))?;
        tx.commit().await?;
        let pending = std::mem::take(&mut self.pending_attribute_current);
        self.attribute_current_queue.publish(pending).await;
        Ok(())
    }
}

impl Drop for UnitOfWork {
    fn drop(&mut self) {
        if self.tx.is_some() {
            tracing::debug!(event = "uow_rollback");
        }
    }
}
