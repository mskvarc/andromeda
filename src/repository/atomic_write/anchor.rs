//! The PostgreSQL anchor: one transaction carrying every part of a set of
//! atomic writes.
//!
//! This store owns all four parts, so nothing is deferred. What it does own is
//! the fusion decision the callers used to make for themselves: how many
//! statements a set of writes becomes, and whether the insert goes through
//! `UNNEST` binds or a binary `COPY` into a staging table.

use crate::{
    core::{config::BulkWriteMode, error::AndromedaError},
    repository::atomic_write::{
        columns::{columns_for, expectations},
        entity_group::{EntityGroups, IndexedWrite, RowReturned, delete_outcome, group, insert_outcome, update_outcome},
        history,
    },
    service::entity::{
        anchor_store::AnchorStore,
        atomic_write::{ALL_WRITE_PARTS, AtomicWrite, WritePart},
        write_outcome::WriteOutcome,
    },
};
use andromeda_postgresql_context_store::{
    attribute_current::queue::AttributeCurrentQueue,
    entity::{copy::create_batch_via_copy_in_tx, deleted::DeletedEntity, repo::EntityRepository},
    outbox::repo::{OutboxInsertRow, OutboxRepository},
    pending_side_effects::{
        payload::{PersistedBatchSideEffectJob, PersistedChangeEvent, PersistedSysAttrs, PersistedTemporalEntity},
        repo::PendingSideEffectsRepository,
    },
};
use andromeda_postgresql_query::history_batch_entry::HistoryBatchEntry;
use andromeda_postgresql_temporal_store::recorder::TemporalRecorder;
use async_trait::async_trait;
use serde_json::Value;
use sqlx::{Postgres, Transaction, postgres::PgPool};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// The transaction one set of atomic writes runs in.
///
/// Holds the history rows written so far because they must not reach the
/// `attribute_current` projection until the commit that makes them visible has
/// succeeded. On drop without a commit, sqlx rolls the transaction back and the
/// rows are discarded with it.
pub struct AnchoredTransaction {
    tx: Option<Transaction<'static, Postgres>>,
    pending_attribute_current: Vec<HistoryBatchEntry>,
}

impl AnchoredTransaction {
    fn tx_mut(&mut self) -> Result<&mut Transaction<'static, Postgres>, AndromedaError> {
        self.tx
            .as_mut()
            .ok_or_else(|| AndromedaError::internal("atomic write transaction used after commit".to_string()))
    }
}

impl Drop for AnchoredTransaction {
    fn drop(&mut self) {
        if self.tx.is_some() {
            tracing::debug!(event = "atomic_write_rollback");
        }
    }
}

/// The PostgreSQL context store, seen as the anchor of an atomic write.
#[derive(Debug)]
pub struct PostgresAnchorStore {
    pool: PgPool,
    recorder: Arc<TemporalRecorder>,
    bulk_write_mode: BulkWriteMode,
    attribute_current_queue: AttributeCurrentQueue,
}

impl PostgresAnchorStore {
    /// Anchor atomic writes in the given tenant pool.
    pub fn new(
        pool: &PgPool,
        recorder: Arc<TemporalRecorder>,
        bulk_write_mode: BulkWriteMode,
        attribute_current_queue: AttributeCurrentQueue,
    ) -> PostgresAnchorStore {
        PostgresAnchorStore {
            pool: pool.clone(), // clone: Arc ref-count bump
            recorder,
            bulk_write_mode,
            attribute_current_queue,
        }
    }

    /// Insert every write in `inserts` with one statement, and report what each
    /// one's precondition makes of the result.
    async fn apply_inserts(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        inserts: &[IndexedWrite<'_>],
        outcomes: &mut [Option<WriteOutcome>],
    ) -> Result<(), AndromedaError> {
        if inserts.is_empty() {
            return Ok(());
        }
        let columns = columns_for(inserts)?;
        let inserted = match self.bulk_write_mode {
            BulkWriteMode::Unnest => EntityRepository::create_batch_in_tx(tx, &columns).await?,
            BulkWriteMode::CopyStaging => create_batch_via_copy_in_tx(tx, &columns).await?,
        };
        let inserted: HashSet<String> = inserted.into_iter().collect();
        for (index, write) in inserts {
            let returned = returned_flag(&inserted, &write.entity_id);
            outcomes[*index] = Some(insert_outcome(write.precondition, returned));
        }
        Ok(())
    }

    /// Overwrite every write in `updates` with one statement.
    async fn apply_updates(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        updates: &[IndexedWrite<'_>],
        outcomes: &mut [Option<WriteOutcome>],
    ) -> Result<(), AndromedaError> {
        if updates.is_empty() {
            return Ok(());
        }
        let columns = columns_for(updates)?;
        let expected = expectations(updates);
        let saved: HashSet<String> = EntityRepository::save_batch_in_tx(tx, &columns, &expected).await?.into_iter().collect();
        for (index, write) in updates {
            let returned = returned_flag(&saved, &write.entity_id);
            outcomes[*index] = Some(update_outcome(write.precondition, returned));
        }
        Ok(())
    }

    /// Remove every write in `deletes` with one statement, handing each one
    /// back what its row held.
    async fn apply_deletes(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        deletes: &[IndexedWrite<'_>],
        outcomes: &mut [Option<WriteOutcome>],
    ) -> Result<(), AndromedaError> {
        if deletes.is_empty() {
            return Ok(());
        }
        let ids: Vec<String> = deletes.iter().map(|(_, write)| write.entity_id.to_string()).collect();
        let expected = expectations(deletes);
        let mut deleted: HashMap<String, DeletedEntity> = EntityRepository::delete_batch_in_tx(tx, &ids, &expected)
            .await?
            .into_iter()
            .map(|entity| (entity.id.to_string(), entity)) // the identifier keys the map as text and stays typed on the value the caller receives
            .collect();
        for (index, write) in deletes {
            outcomes[*index] = Some(delete_outcome(write.precondition, deleted.remove(write.entity_id.as_ref())));
        }
        Ok(())
    }
}

/// Whether a fused statement returned `id`.
fn returned_flag(returned: &HashSet<String>, id: &str) -> RowReturned {
    if returned.contains(id) { RowReturned::Yes } else { RowReturned::No }
}

#[async_trait]
impl AnchorStore for PostgresAnchorStore {
    type Transaction = AnchoredTransaction;

    fn native_parts(&self) -> &[WritePart] {
        &ALL_WRITE_PARTS
    }

    async fn begin(&self) -> Result<AnchoredTransaction, AndromedaError> {
        Ok(AnchoredTransaction {
            tx: Some(self.pool.begin().await?),
            pending_attribute_current: Vec::new(),
        })
    }

    async fn apply_entities(&self, tx: &mut AnchoredTransaction, writes: &[AtomicWrite]) -> Result<Vec<WriteOutcome>, AndromedaError> {
        let EntityGroups {
            inserts,
            updates,
            deletes,
            untouched,
        } = group(writes);

        let mut outcomes: Vec<Option<WriteOutcome>> = writes.iter().map(|_| None).collect();
        for index in untouched {
            outcomes[index] = Some(WriteOutcome::NoEntityChange);
        }

        let inner = tx.tx_mut()?;
        self.apply_inserts(inner, &inserts, &mut outcomes).await?;
        self.apply_updates(inner, &updates, &mut outcomes).await?;
        self.apply_deletes(inner, &deletes, &mut outcomes).await?;

        outcomes
            .into_iter()
            .enumerate()
            .map(|(index, outcome)| outcome.ok_or_else(|| AndromedaError::internal(format!("atomic write {index} produced no outcome"))))
            .collect()
    }

    async fn apply_history(&self, tx: &mut AnchoredTransaction, landed: &[&AtomicWrite]) -> Result<(), AndromedaError> {
        if landed.iter().all(|write| write.history.is_empty()) {
            return Ok(());
        }
        let recorder = Arc::clone(&self.recorder); // clone: Arc ref-count bump, so the transaction can be borrowed mutably alongside it
        let written = history::apply(&recorder, tx.tx_mut()?, landed).await?;
        tx.pending_attribute_current.extend(written);
        Ok(())
    }

    async fn apply_outbox(&self, tx: &mut AnchoredTransaction, landed: &[&AtomicWrite]) -> Result<(), AndromedaError> {
        let entries: Vec<_> = landed.iter().flat_map(|write| write.outbox.iter()).collect();
        if entries.is_empty() {
            return Ok(());
        }
        let null = serde_json::Value::Null;
        let trigger_strs: Vec<String> = entries.iter().map(|e| e.trigger.to_string()).collect();
        let attr_strs: Vec<Vec<String>> = entries.iter().map(|e| e.changed_attrs.iter().map(ToString::to_string).collect()).collect();
        let rows: Vec<OutboxInsertRow<'_>> = entries
            .iter()
            .zip(&trigger_strs)
            .zip(&attr_strs)
            .map(|((e, trig), attrs)| OutboxInsertRow {
                subscription_id: e.subscription_id.as_str(),
                entity_id: e.entity_id.as_str(),
                trigger: trig,
                entity_snapshot: e.entity_snapshot.as_ref().unwrap_or(&null),
                previous_snapshot: &e.previous_snapshot,
                changed_attrs: attrs,
            })
            .collect();
        OutboxRepository::insert_batch_raw_in_tx(tx.tx_mut()?, &rows).await?;
        Ok(())
    }

    async fn apply_side_effects(&self, tx: &mut AnchoredTransaction, landed: &[&AtomicWrite]) -> Result<(), AndromedaError> {
        let mut merged = PersistedBatchSideEffectJob {
            temporal_entities: Vec::new(),
            change_events: Vec::new(),
        };
        for write in landed {
            let Some(job) = &write.side_effects else {
                continue;
            };
            merged
                .temporal_entities
                .extend(job.temporal_entities.iter().map(|(id, expanded)| PersistedTemporalEntity {
                    id: id.to_string(),
                    expanded: Value::clone(expanded),
                }));
            merged.change_events.extend(job.change_events.iter().map(|ce| PersistedChangeEvent {
                entity_id: ce.entity_id.to_string(),
                entity_types: ce.entity_types.iter().map(ToString::to_string).collect(),
                trigger_reason: ce.trigger_reason.to_string(),
                changed_attributes: ce.changed_attributes.iter().map(ToString::to_string).collect(),
                occurred_at: ce.occurred_at,
                entity_snapshot: ce.entity_snapshot.as_deref().cloned(),
                previous_attributes: ce.previous_attributes.as_deref().cloned(),
                sys_attrs_snapshot: ce.sys_attrs_snapshot.as_ref().map(|s| PersistedSysAttrs {
                    created_at: s.created_at,
                    modified_at: s.modified_at,
                    expires_at: s.expires_at,
                }),
                origin: match ce.origin {
                    crate::event::change_event::ChangeOrigin::Local => "local".to_string(),
                    crate::event::change_event::ChangeOrigin::Forwarded => "forwarded".to_string(),
                },
            }));
        }
        if merged.temporal_entities.is_empty() && merged.change_events.is_empty() {
            return Ok(());
        }
        // One queue row for the whole set. The atomic unit stays one entity;
        // fusing their jobs is this store's decision and keeps the row count
        // where a per-chunk enqueue put it.
        PendingSideEffectsRepository::enqueue_in_tx(tx.tx_mut()?, &merged).await?;
        Ok(())
    }

    async fn commit(&self, mut tx: AnchoredTransaction) -> Result<(), AndromedaError> {
        let inner = tx
            .tx
            .take()
            .ok_or_else(|| AndromedaError::internal("atomic write transaction committed twice".to_string()))?;

        // The reservation straddles the commit deliberately. A projection *fed*
        // before the commit would answer a `q` filter with a value no reader can
        // see; a sequence *claimed* only after it would leave a window in which
        // the entity row is visible, the high-water mark has not moved, and a
        // concurrent query's barrier concludes there is nothing to wait for. So
        // the sequence is claimed here and the rows are handed over below.
        let pending = std::mem::take(&mut tx.pending_attribute_current);
        let reserved = self.attribute_current_queue.reserve(pending);

        match inner.commit().await {
            Ok(()) => {
                // The queue is bounded, so a materializer that has fallen behind
                // delays this call rather than growing without limit -- and by
                // this point no database resource is held.
                if let Some(reserved) = reserved {
                    reserved.enqueue().await;
                }
                Ok(())
            }
            Err(error) => {
                if let Some(reserved) = reserved {
                    reserved.abandon().await;
                }
                Err(error.into())
            }
        }
    }
}
