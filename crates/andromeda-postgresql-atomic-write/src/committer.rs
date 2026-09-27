//! Native PostgreSQL atomic context commits.

use crate::{
    columns::{columns_for, expectations},
    commit_refusal::{context_store, database, temporal},
    consequences::{ConsequenceWrite, notification_events, side_effects},
    contention_retry::rerun_while_contended,
    deferred_history,
    entity_group::{EntityGroups, IndexedWrite, RowReturned, delete_outcome, group, insert_outcome, update_outcome},
    history,
    history_mode::{HistoryPlacement, TemporalHistoryCommitMode},
    job_settlement::settle,
    name_resolution::{ResolvedWriteNames, resolve_attribute_names, resolve_write_names},
    projection,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_consequence_publisher::CommitConsequencePublisher,
    commit_replay::CommitReplay,
    committer::{CommitFuture, ContextCommitter, consequences_land, validate_submission},
    deleted_entity::DeletedEntity,
    error::CommitError,
    job_lease::JobLease,
    write_ordinal::WriteOrdinal,
    write_outcome::WriteOutcome,
    write_record::WriteRecord,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_postgresql_codes::entity_key::EntityKey;
use andromeda_postgresql_context_store::entity::{copy::create_batch_via_copy_in_tx, repo::EntityRepository, written_entity::WrittenEntity};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use andromeda_temporal::append_intent::TemporalAppendIntent;
use chrono::Utc;
use sqlx::{PgPool, Postgres, Transaction};
use std::{collections::HashMap, sync::Arc};

/// PostgreSQL statement strategy used for fused entity inserts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityInsertMode {
    /// Bind parallel arrays and insert from `UNNEST`.
    Unnest,
    /// Stream binary rows into a staging table before insertion.
    CopyStaging,
}

/// What one commit's entity statements produced.
struct EntityMutations {
    /// Submission-order outcome of every write in the commit.
    outcomes: Vec<WriteOutcome>,
    /// Surrogate key of every entity whose row this commit wrote.
    ///
    /// A delete contributes nothing: the projection's single foreign key
    /// cascades from `entities`, so a deleted entity's rows go with it.
    keys: HashMap<Uri, EntityKey>,
}

/// Atomic context committer backed by one tenant's PostgreSQL database.
pub struct PostgresAtomicCommitter {
    /// Tenant pool whose connection owns the complete transaction.
    pool: PgPool,
    /// Native or deferred temporal history behavior.
    history_mode: TemporalHistoryCommitMode,
    /// Fused insert statement strategy.
    entity_insert_mode: EntityInsertMode,
    /// Dictionary the projection's attribute keys are minted through.
    attribute_names: Arc<dyn NameLookup<AttributeName>>,
    /// Dictionary the entity rows' type keys are minted through.
    entity_type_names: Arc<dyn NameLookup<EntityTypeName>>,
    /// Edge notification woken when a commit writes notification events or side effects.
    consequence_publisher: Arc<dyn CommitConsequencePublisher>,
}

impl PostgresAtomicCommitter {
    /// Construct the native PostgreSQL commit implementation for one tenant.
    pub fn new(
        pool: &PgPool,
        history_mode: TemporalHistoryCommitMode,
        entity_insert_mode: EntityInsertMode,
        attribute_names: Arc<dyn NameLookup<AttributeName>>,
        entity_type_names: Arc<dyn NameLookup<EntityTypeName>>,
        consequence_publisher: Arc<dyn CommitConsequencePublisher>,
    ) -> PostgresAtomicCommitter {
        PostgresAtomicCommitter {
            pool: pool.clone(), // clone: SQLx pools are shared reference-counted handles.
            history_mode,
            entity_insert_mode,
            attribute_names,
            entity_type_names,
            consequence_publisher,
        }
    }

    /// Apply every current-state mutation and preserve submission-order
    /// outcomes.
    async fn apply_entities(&self, tx: &mut Transaction<'_, Postgres>, target: &ContextTarget, writes: &[AtomicWrite]) -> Result<EntityMutations, CommitError> {
        let EntityGroups {
            inserts,
            updates,
            deletes,
            untouched,
        } = group(writes);
        let mut outcomes: Vec<Option<WriteOutcome>> = writes.iter().map(|_| None).collect();
        let mut keys: HashMap<Uri, EntityKey> = HashMap::new();
        for &index in &untouched {
            outcomes[index] = Some(WriteOutcome::NoEntityChange);
        }
        // Minted before the fused statements run: both bind the key arrays.
        let names = resolve_write_names(tx, writes, self.entity_type_names.as_ref(), self.attribute_names.as_ref()).await?;
        self.apply_inserts(tx, target, &inserts, &names, &mut outcomes, &mut keys).await?;
        self.apply_updates(tx, target, &updates, &names, &mut outcomes, &mut keys).await?;
        self.apply_deletes(tx, target, &deletes, &mut outcomes).await?;
        resolve_untouched_keys(tx, target, writes, &untouched, &mut keys).await?;
        let outcomes = outcomes
            .into_iter()
            .enumerate()
            .map(|(index, outcome)| {
                outcome.ok_or_else(|| CommitError::Invariant {
                    detail: format!("atomic write {index} produced no outcome"),
                })
            })
            .collect::<Result<Vec<WriteOutcome>, CommitError>>()?;
        Ok(EntityMutations { outcomes, keys })
    }

    /// Insert the grouped entity rows with the configured fused statement.
    async fn apply_inserts(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        inserts: &[IndexedWrite<'_>],
        names: &ResolvedWriteNames,
        outcomes: &mut [Option<WriteOutcome>],
        keys: &mut HashMap<Uri, EntityKey>,
    ) -> Result<(), CommitError> {
        if inserts.is_empty() {
            return Ok(());
        }
        let columns = columns_for(inserts, names)?;
        let inserted = match self.entity_insert_mode {
            EntityInsertMode::Unnest => EntityRepository::create_batch_in_tx(tx, target, &columns).await,
            EntityInsertMode::CopyStaging => create_batch_via_copy_in_tx(tx, target, &columns).await,
        }
        .map_err(context_store)?;
        record_keys(keys, inserted);
        for (index, write) in inserts {
            outcomes[*index] = Some(insert_outcome(write.precondition, returned_flag(keys, write.entity_id.as_ref())));
        }
        Ok(())
    }

    /// Update the grouped entity rows with one fused statement.
    async fn apply_updates(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        updates: &[IndexedWrite<'_>],
        names: &ResolvedWriteNames,
        outcomes: &mut [Option<WriteOutcome>],
        keys: &mut HashMap<Uri, EntityKey>,
    ) -> Result<(), CommitError> {
        if updates.is_empty() {
            return Ok(());
        }
        let columns = columns_for(updates, names)?;
        let expected = expectations(updates);
        let saved = EntityRepository::save_batch_in_tx(tx, target, &columns, &expected)
            .await
            .map_err(context_store)?;
        record_keys(keys, saved);
        for (index, write) in updates {
            outcomes[*index] = Some(update_outcome(write.precondition, returned_flag(keys, write.entity_id.as_ref())));
        }
        Ok(())
    }

    /// Delete the grouped entity rows and retain their pre-deletion state.
    async fn apply_deletes(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
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
        let mut snapshots: HashMap<Uri, DeletedEntity> = EntityRepository::delete_batch_in_tx(tx, target, &ids, &expected)
            .await
            .map_err(context_store)?
            .into_iter()
            .map(|entity| (entity.id.clone(), entity)) // clone: both the lookup key and returned snapshot own the identifier.
            .collect();
        for (index, write) in deletes {
            outcomes[*index] = Some(delete_outcome(write.precondition, snapshots.remove(write.entity_id.as_ref())));
        }
        Ok(())
    }
}

/// Fetch the surrogate keys of untouched writes that still carry history.
///
/// A write with `EntityMutation::None` runs no entity statement, so no
/// statement returns its key — yet its history (a replayed batch of
/// observations, say) is projected against exactly that key. Without this
/// lookup the projection silently skips every such entity. Only writes with
/// history are resolved; a bare no-op has nothing to project.
async fn resolve_untouched_keys(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    writes: &[AtomicWrite],
    untouched: &[usize],
    keys: &mut HashMap<Uri, EntityKey>,
) -> Result<(), CommitError> {
    let ids: Vec<Uri> = untouched
        .iter()
        .map(|&index| &writes[index])
        .filter(|write| !write.history.is_empty())
        .map(|write| write.entity_id.as_ref().clone()) // clone: the SQL bind list owns identifiers borrowed from writes.
        .collect();
    if ids.is_empty() {
        return Ok(());
    }
    let resolved = EntityRepository::keys_by_ids_in_tx(tx, target, &ids).await.map_err(context_store)?;
    record_keys(keys, resolved);
    Ok(())
}

/// Whether a fused statement returned an entity identifier.
fn returned_flag(returned: &HashMap<Uri, EntityKey>, id: &Uri) -> RowReturned {
    if returned.contains_key(id) { RowReturned::Yes } else { RowReturned::No }
}

/// Remember the surrogate key of every entity a fused statement wrote.
fn record_keys(keys: &mut HashMap<Uri, EntityKey>, written: Vec<WrittenEntity>) {
    keys.extend(written.into_iter().map(|entity| (entity.id, entity.key)));
}

/// Interpret every landed write's temporal consequences exactly once, and
/// report the ordinal one past the last one this phase consumed.
///
/// History and the current-attribute projection are two readings of the same
/// mutation, so they read one interpretation rather than each making their
/// own. Every entity a commit lands shares the commit's one write identity, so
/// their intents cannot each restart their own ordinal count at zero: every
/// intent draws its entries' ordinals from the same running counter, seeded at
/// `base`, and the counter advances past exactly the entries that intent used.
fn interpret(landed: &[&AtomicWrite], record: &WriteRecord, base: WriteOrdinal) -> Result<(Vec<TemporalAppendIntent>, WriteOrdinal), CommitError> {
    let mut next = base;
    let mut intents = Vec::with_capacity(landed.len());
    for write in landed {
        let entity_types = write.recorded_entity_types()?;
        let Some(intent) = TemporalAppendIntent::from_history(&write.entity_id, &entity_types, &write.history, *record, next).map_err(temporal)? else {
            continue;
        };
        let advance = u32::try_from(intent.entries.len()).map_err(|error| CommitError::Invariant {
            detail: format!(
                "temporal intent for entity {} carries more entries than an ordinal can number: {error}",
                write.entity_id
            ),
        })?;
        next = next.advanced_by(advance).map_err(|error| CommitError::Invariant {
            detail: format!("temporal consequence ordinal overflowed past entity {}: {error}", write.entity_id),
        })?;
        intents.push(intent);
    }
    Ok((intents, next))
}

impl PostgresAtomicCommitter {
    /// Commit every write under one write identity, numbering every temporal,
    /// notification and side-effect consequence from `base`.
    ///
    /// Every durable consequence of this commit — every landed entity's
    /// temporal history entries, its notification events, and its side-effect
    /// job — shares the one write identity and ingestion instant carried by
    /// `record`, and draws its ordinals from the one running counter that
    /// starts at `base`. `commit` mints a fresh record and starts that counter
    /// at zero; `commit_replayed` reuses a durably stored record and resumes
    /// the counter at its stored `base_ordinal`, so a replay's consequences
    /// belong to the write that deferred them and follow the ones that write
    /// already committed.
    ///
    /// One call is one transaction. A transaction the datastore rolls back as
    /// a deadlock or serialization victim ends in
    /// [`CommitError::TransactionContended`] having left nothing behind, which
    /// is what lets [`rerun_while_contended`] run the identical call again
    /// under the same record and base.
    ///
    /// A replay also removes the job it `settles` in that one transaction, as
    /// the transaction's first statement and under the job's lease (see
    /// [`settle`]): a replay whose lease had already lapsed ends in
    /// [`CommitError::ReplayLeaseLost`] before it wrote anything, and one that
    /// held it keeps the job row-locked until it commits, however long the
    /// replay runs.
    async fn commit_under(
        &self,
        target: &ContextTarget,
        record: WriteRecord,
        base: WriteOrdinal,
        writes: &[AtomicWrite],
        settles: Option<JobLease>,
    ) -> Result<Vec<WriteOutcome>, CommitError> {
        if writes.is_empty() && settles.is_none() {
            return Ok(Vec::new());
        }
        validate_submission(writes)?;
        let mut tx = self.pool.begin().await.map_err(database)?;
        // Every fallible phase below borrows this owned transaction. SQLx
        // rolls it back when an early `?` drops it, so no entity mutation
        // can survive a failed history, projection, notification or side-effect
        // write, and a settled job is restored with its lease.
        if let Some(lease) = settles {
            settle(&mut tx, lease).await?;
        }
        if writes.is_empty() {
            tx.commit().await.map_err(database)?;
            return Ok(Vec::new());
        }
        let EntityMutations { outcomes, keys } = self.apply_entities(&mut tx, target, writes).await?;
        let landed: Vec<&AtomicWrite> = writes
            .iter()
            .zip(&outcomes)
            .filter(|(_, outcome)| consequences_land(outcome))
            .map(|(write, _)| write)
            .collect();
        // One interpretation of the mutation's temporal consequences serves
        // both the history phase and the projection phase, so the two can
        // never disagree about what the write said.
        let (intents, next) = interpret(&landed, &record, base)?;
        let placement = self.history_mode.placement(target);
        match placement {
            HistoryPlacement::Inline(recorder) => history::apply(recorder, &mut tx, target, &record, &intents).await?,
            HistoryPlacement::Queued(_) => deferred_history::apply(&mut tx, &intents).await?,
        }
        // Minted from the interpreted intents so the projection references
        // only keys this transaction holds.
        let attribute_keys = resolve_attribute_names(&mut tx, &intents, self.attribute_names.as_ref()).await?;
        projection::apply(&mut tx, target, &intents, &keys, &attribute_keys).await?;
        let (next, notifications) = notification_events::apply(&mut tx, target, &landed, &record, next, self.attribute_names.as_ref()).await?;
        let side_effects = side_effects::apply(&mut tx, target, &landed, &record, next).await?;

        tx.commit().await.map_err(database)?;
        tracing::debug!(write_id = %record.write_id(), writes = writes.len(), "atomic context write committed");
        if !intents.is_empty()
            && let HistoryPlacement::Queued(publisher) = placement
        {
            publisher.intents_available();
        }
        // Wake the consequence drains only for the commit that enqueued their
        // work, so an idle tenant's widened fallback stays quiet.
        if notifications == ConsequenceWrite::Written {
            self.consequence_publisher.notifications_available();
        }
        if side_effects == ConsequenceWrite::Written {
            self.consequence_publisher.side_effects_available();
        }
        Ok(outcomes)
    }
}

impl ContextCommitter for PostgresAtomicCommitter {
    fn commit(&self, target: &ContextTarget, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        let target = target.clone();
        Box::pin(async move {
            let record = WriteRecord::mint(Utc::now());
            rerun_while_contended(|| self.commit_under(&target, record, WriteOrdinal::at(0), &writes, None)).await
        })
    }

    fn commit_replayed(&self, target: &ContextTarget, replay: CommitReplay, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        let target = target.clone();
        Box::pin(async move { rerun_while_contended(|| self.commit_under(&target, replay.record, replay.base_ordinal, &writes, Some(replay.lease))).await })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        committer::{interpret, record_keys, returned_flag},
        entity_group::RowReturned,
    };
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        entity_mutation::EntityMutation,
        history_record::HistoryRecord,
        write_id::WriteId,
        write_ordinal::WriteOrdinal,
        write_record::WriteRecord,
    };
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_postgresql_codes::entity_key::EntityKey;
    use andromeda_postgresql_context_store::entity::written_entity::WrittenEntity;
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use std::{collections::HashMap, sync::Arc};

    fn entity(id: &str, key: i64) -> WrittenEntity {
        WrittenEntity {
            key: EntityKey::try_from(key).expect("a positive surrogate key"),
            id: id.parse::<Uri>().expect("valid entity URI"),
        }
    }

    #[test]
    fn returned_flag_reports_identifier_membership() {
        let mut keys = HashMap::new();
        record_keys(&mut keys, vec![entity("urn:ngsi-ld:A", 1)]);
        let present = "urn:ngsi-ld:A".parse::<Uri>().expect("valid entity URI");
        let absent = "urn:ngsi-ld:B".parse::<Uri>().expect("valid entity URI");
        assert!(matches!(returned_flag(&keys, &present), RowReturned::Yes));
        assert!(matches!(returned_flag(&keys, &absent), RowReturned::No));
    }

    /// The projection is written against the surrogate key, so a write whose
    /// row the statement did not return contributes no key and therefore no
    /// projection row: there is no path by which a skipped write reaches the
    /// projection.
    #[test]
    fn only_the_entities_a_statement_wrote_carry_a_surrogate_key_into_the_projection() {
        let mut keys = HashMap::new();
        record_keys(&mut keys, vec![entity("urn:ngsi-ld:A", 1), entity("urn:ngsi-ld:C", 3)]);
        let mut named: Vec<String> = keys.keys().map(|id| id.as_str().to_owned()).collect();
        named.sort();
        pretty_assertions::assert_eq!(named, vec!["urn:ngsi-ld:A".to_owned(), "urn:ngsi-ld:C".to_owned()]);
        pretty_assertions::assert_eq!(
            keys.get(&"urn:ngsi-ld:C".parse::<Uri>().expect("valid entity URI"))
                .map(|key| EntityKey::get(*key)),
            Some(3)
        );
    }

    fn observation_write(entity_id: &str) -> AtomicWrite {
        let uri = entity_id.parse::<Uri>().expect("valid entity URI");
        let payload = Arc::new(json!([{
            "@id": entity_id,
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]
            }]
        }]));
        AtomicWrite::builder()
            .entity_id(Arc::new(uri))
            .entity(EntityMutation::None)
            .history(vec![HistoryRecord::Observations(payload)])
            .build()
    }

    /// Two entities landed by the same commit must not restart their ordinal
    /// count at zero: the second entity's entries continue the running counter
    /// where the first entity's left off.
    #[test]
    fn interpret_assigns_each_landed_entitys_intent_a_base_that_continues_the_previous_ones_ordinals() {
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let record = WriteRecord::restored(WriteId::mint(), recorded_at);
        let first = observation_write("urn:ngsi-ld:A");
        let second = observation_write("urn:ngsi-ld:B");
        let landed = [&first, &second];

        let (intents, next) = interpret(&landed, &record, WriteOrdinal::at(0)).expect("both entities interpret cleanly");

        pretty_assertions::assert_eq!(intents.len(), 2);
        pretty_assertions::assert_eq!(intents[0].base_ordinal, WriteOrdinal::at(0));
        pretty_assertions::assert_eq!(intents[1].base_ordinal, WriteOrdinal::at(1));
        pretty_assertions::assert_eq!(next, WriteOrdinal::at(2));
    }

    /// A replay resumes the running counter at the base it was handed rather
    /// than at zero.
    #[test]
    fn interpret_resumes_the_running_counter_at_a_non_zero_base() {
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let record = WriteRecord::restored(WriteId::mint(), recorded_at);
        let write = observation_write("urn:ngsi-ld:A");
        let landed = [&write];

        let (intents, next) = interpret(&landed, &record, WriteOrdinal::at(5)).expect("the entity interprets cleanly");

        pretty_assertions::assert_eq!(intents[0].base_ordinal, WriteOrdinal::at(5));
        pretty_assertions::assert_eq!(next, WriteOrdinal::at(6));
    }
}
