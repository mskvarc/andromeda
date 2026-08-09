//! Native PostgreSQL atomic context commits.

use crate::{
    columns::{columns_for, expectations},
    entity_group::{EntityGroups, IndexedWrite, RowReturned, delete_outcome, group, insert_outcome, update_outcome},
    history,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    committer::{CommitFuture, ContextCommitter, consequences_land, validate_submission},
    deleted_entity::DeletedEntity,
    error::CommitError,
    write_outcome::WriteOutcome,
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_context_store::{
    attribute_current::queue::AttributeCurrentQueue,
    entity::{copy::create_batch_via_copy_in_tx, repo::EntityRepository},
    outbox::repo::{OutboxInsertRecord, OutboxRepository},
    pending_side_effects::{
        payload::{PersistedBatchSideEffectJob, PersistedChangeEvent, PersistedSysAttrs, PersistedTemporalEntity},
        repo::PendingSideEffectsRepository,
    },
};
use andromeda_postgresql_query::history_batch_entry::HistoryBatchEntry;
use andromeda_postgresql_temporal_store::recorder::TemporalRecorder;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    sync::Arc,
};

/// PostgreSQL statement strategy used for fused entity inserts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityInsertMode {
    /// Bind parallel arrays and insert from `UNNEST`.
    Unnest,
    /// Stream binary rows into a staging table before insertion.
    CopyStaging,
}

/// Atomic context committer backed by one tenant's PostgreSQL database.
#[derive(Debug)]
pub struct PostgresAtomicCommitter {
    /// Tenant pool whose connection owns the complete transaction.
    pool: PgPool,
    /// Native PostgreSQL temporal-history recorder used inside that transaction.
    recorder: Arc<TemporalRecorder>,
    /// Fused insert statement strategy.
    entity_insert_mode: EntityInsertMode,
    /// Post-commit publisher for the current-attribute projection.
    attribute_current_queue: AttributeCurrentQueue,
}

impl PostgresAtomicCommitter {
    /// Construct the native PostgreSQL commit implementation for one tenant.
    pub fn new(
        pool: &PgPool,
        recorder: Arc<TemporalRecorder>,
        entity_insert_mode: EntityInsertMode,
        attribute_current_queue: AttributeCurrentQueue,
    ) -> PostgresAtomicCommitter {
        PostgresAtomicCommitter {
            pool: pool.clone(), // clone: SQLx pools are shared reference-counted handles.
            recorder,
            entity_insert_mode,
            attribute_current_queue,
        }
    }

    /// Apply every current-state mutation and preserve submission-order
    /// outcomes.
    async fn apply_entities(&self, tx: &mut Transaction<'_, Postgres>, writes: &[AtomicWrite]) -> Result<Vec<WriteOutcome>, CommitError> {
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
        self.apply_inserts(tx, &inserts, &mut outcomes).await?;
        self.apply_updates(tx, &updates, &mut outcomes).await?;
        self.apply_deletes(tx, &deletes, &mut outcomes).await?;
        outcomes
            .into_iter()
            .enumerate()
            .map(|(index, outcome)| {
                outcome.ok_or_else(|| CommitError::Invariant {
                    detail: format!("atomic write {index} produced no outcome"),
                })
            })
            .collect()
    }

    /// Insert the grouped entity rows with the configured fused statement.
    async fn apply_inserts(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        inserts: &[IndexedWrite<'_>],
        outcomes: &mut [Option<WriteOutcome>],
    ) -> Result<(), CommitError> {
        if inserts.is_empty() {
            return Ok(());
        }
        let columns = columns_for(inserts)?;
        let inserted = match self.entity_insert_mode {
            EntityInsertMode::Unnest => EntityRepository::create_batch_in_tx(tx, &columns).await,
            EntityInsertMode::CopyStaging => create_batch_via_copy_in_tx(tx, &columns).await,
        }
        .map_err(persistence)?;
        let inserted: HashSet<Uri> = inserted.into_iter().collect();
        for (index, write) in inserts {
            outcomes[*index] = Some(insert_outcome(write.precondition, returned_flag(&inserted, write.entity_id.as_ref())));
        }
        Ok(())
    }

    /// Update the grouped entity rows with one fused statement.
    async fn apply_updates(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        updates: &[IndexedWrite<'_>],
        outcomes: &mut [Option<WriteOutcome>],
    ) -> Result<(), CommitError> {
        if updates.is_empty() {
            return Ok(());
        }
        let columns = columns_for(updates)?;
        let expected = expectations(updates);
        let saved: HashSet<Uri> = EntityRepository::save_batch_in_tx(tx, &columns, &expected)
            .await
            .map_err(persistence)?
            .into_iter()
            .collect();
        for (index, write) in updates {
            outcomes[*index] = Some(update_outcome(write.precondition, returned_flag(&saved, write.entity_id.as_ref())));
        }
        Ok(())
    }

    /// Delete the grouped entity rows and retain their pre-deletion state.
    async fn apply_deletes(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        deletes: &[IndexedWrite<'_>],
        outcomes: &mut [Option<WriteOutcome>],
    ) -> Result<(), CommitError> {
        if deletes.is_empty() {
            return Ok(());
        }
        let ids: Vec<Uri> = deletes
            .iter()
            .map(|(_, write)| write.entity_id.as_ref().clone()) // clone: the SQL bind list owns identifiers borrowed from writes.
            .collect();
        let expected = expectations(deletes);
        let mut deleted: HashMap<Uri, DeletedEntity> = EntityRepository::delete_batch_in_tx(tx, &ids, &expected)
            .await
            .map_err(persistence)?
            .into_iter()
            .map(|entity| (entity.id.clone(), entity)) // clone: both the lookup key and returned snapshot own the identifier.
            .collect();
        for (index, write) in deletes {
            outcomes[*index] = Some(delete_outcome(write.precondition, deleted.remove(write.entity_id.as_ref())));
        }
        Ok(())
    }

    /// Persist notification outbox entries for landed writes.
    async fn apply_outbox(tx: &mut Transaction<'_, Postgres>, landed: &[&AtomicWrite]) -> Result<(), CommitError> {
        let entries: Vec<_> = landed.iter().flat_map(|write| write.outbox.iter()).collect();
        if entries.is_empty() {
            return Ok(());
        }
        let null = Value::Null;
        let rows: Vec<OutboxInsertRecord<'_>> = entries
            .iter()
            .map(|entry| OutboxInsertRecord {
                subscription_id: &entry.subscription_id,
                entity_id: &entry.entity_id,
                trigger: entry.trigger,
                entity_snapshot: match entry.entity_snapshot.as_ref() {
                    Some(snapshot) => snapshot,
                    None => &null,
                },
                previous_snapshot: &entry.previous_snapshot,
                changed_attrs: &entry.changed_attrs,
            })
            .collect();
        OutboxRepository::insert_batch_in_tx(tx, &rows).await.map_err(persistence)?;
        Ok(())
    }

    /// Persist asynchronous side-effect jobs for landed writes.
    async fn apply_side_effects(tx: &mut Transaction<'_, Postgres>, landed: &[&AtomicWrite]) -> Result<(), CommitError> {
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
                    id: id.as_ref().clone(), // clone: the persisted DTO owns the validated entity URI.
                    expanded: Value::clone(expanded),
                }));
            merged.change_events.extend(job.change_events.iter().map(|event| PersistedChangeEvent {
                entity_id: event.entity_id.clone(),       // clone: persistence owns the event identifier.
                entity_types: event.entity_types.clone(), // clone: persistence owns the event type list.
                trigger_reason: event.trigger_reason,
                changed_attributes: event.changed_attributes.clone(), // clone: persistence owns the changed-attribute list.
                occurred_at: event.occurred_at,
                entity_snapshot: event.entity_snapshot.as_deref().cloned(),
                previous_attributes: event.previous_attributes.as_deref().cloned(),
                sys_attrs_snapshot: event.sys_attrs_snapshot.as_ref().map(|snapshot| PersistedSysAttrs {
                    created_at: snapshot.created_at,
                    modified_at: snapshot.modified_at,
                    expires_at: snapshot.expires_at,
                }),
                origin: event.origin,
            }));
        }
        if merged.temporal_entities.is_empty() && merged.change_events.is_empty() {
            return Ok(());
        }
        PendingSideEffectsRepository::enqueue_in_tx(tx, &merged).await.map_err(persistence)?;
        Ok(())
    }
}

/// Convert a concrete PostgreSQL-store failure at the implementation boundary.
fn persistence(error: impl Display) -> CommitError {
    CommitError::Persistence { detail: error.to_string() }
}

/// Whether a fused statement returned an entity identifier.
fn returned_flag(returned: &HashSet<Uri>, id: &Uri) -> RowReturned {
    if returned.contains(id) { RowReturned::Yes } else { RowReturned::No }
}

impl ContextCommitter for PostgresAtomicCommitter {
    fn commit(&self, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        Box::pin(async move {
            if writes.is_empty() {
                return Ok(Vec::new());
            }
            validate_submission(&writes)?;
            let mut tx = self.pool.begin().await.map_err(persistence)?;
            // Every fallible phase below borrows this owned transaction. SQLx
            // rolls it back when an early `?` drops it, so no entity mutation
            // can survive a failed history, outbox or side-effect write.
            let outcomes = self.apply_entities(&mut tx, &writes).await?;
            let landed: Vec<&AtomicWrite> = writes
                .iter()
                .zip(&outcomes)
                .filter(|(_, outcome)| consequences_land(outcome))
                .map(|(write, _)| write)
                .collect();
            let pending_attribute_current: Vec<HistoryBatchEntry> = history::apply(&self.recorder, &mut tx, &landed).await?;
            PostgresAtomicCommitter::apply_outbox(&mut tx, &landed).await?;
            PostgresAtomicCommitter::apply_side_effects(&mut tx, &landed).await?;

            // Claim the projection sequence before commit so a concurrent query
            // cannot observe current state while believing there is no projection
            // work to await. Rows are published only after commit succeeds.
            let reserved = self.attribute_current_queue.reserve(pending_attribute_current);
            match tx.commit().await {
                Ok(()) => {
                    if let Some(reserved) = reserved {
                        reserved.enqueue().await;
                    }
                    Ok(outcomes)
                }
                Err(error) => {
                    if let Some(reserved) = reserved {
                        reserved.abandon().await;
                    }
                    Err(persistence(error))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::committer::{persistence, returned_flag};
    use andromeda_context_commit::error::CommitError;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use std::collections::HashSet;

    #[test]
    fn returned_flag_reports_identifier_membership() {
        let id = "urn:ngsi-ld:A".parse::<Uri>().expect("valid entity URI");
        let returned = HashSet::from([id.clone()]); // clone: both the set and membership query own or borrow the identifier.
        assert!(matches!(returned_flag(&returned, &id), crate::entity_group::RowReturned::Yes));
    }

    #[test]
    fn concrete_store_errors_are_erased_at_the_port_boundary() {
        let error = persistence("database unavailable");
        assert!(matches!(error, CommitError::Persistence { .. }));
    }
}
