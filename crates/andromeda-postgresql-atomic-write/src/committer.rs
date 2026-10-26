//! Native PostgreSQL atomic context commits.

use crate::{
    attempt_failure::AttemptFailure,
    behind_queue,
    columns::{columns_for, expectations},
    commit_refusal::{context_store, database, entity_statement},
    consequences::{
        ConsequenceWrite,
        notification_events,
        numbering::{NumberedConsequences, number_consequences},
        side_effects,
    },
    contention_retry::rerun_while_contended,
    deferred_history,
    entity_group::{EntityGroups, IndexedWrite, RowReturned, delete_outcome, group, insert_outcome, update_outcome},
    history,
    history_mode::{CommitResponse, HistoryPlacement, TemporalHistoryCommitMode},
    job_settlement::settle,
    name_resolution::resolve_write_names,
    write_isolation::IsolatedSubmission,
};
use andromeda_configuration::schema::temporal::core_write_history::CoreWriteHistory;
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_consequence_publisher::CommitConsequencePublisher,
    commit_invariant::CommitInvariant,
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
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_postgresql_attribute_instance::name_keys::NameKeys;
use andromeda_postgresql_context_store::entity::repo::EntityRepository;
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use sqlx::{PgPool, Postgres, Transaction};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// What one commit's entity statements produced.
struct EntityMutations {
    /// Submission-order outcome of every write in the commit.
    outcomes: Vec<WriteOutcome>,
}

/// Atomic context committer backed by one tenant's PostgreSQL database.
pub struct PostgresAtomicCommitter {
    /// Tenant pool whose connection owns the complete transaction.
    pool: PgPool,
    /// Whether a commit records its temporal history inline or queues it.
    history_mode: TemporalHistoryCommitMode,
    /// Whether Core writes add to the Temporal Evolution they change.
    core_write_history: CoreWriteHistory,
    /// Dictionary the attribute name keys of the entity rows are minted
    /// through.
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
        core_write_history: CoreWriteHistory,
        attribute_names: Arc<dyn NameLookup<AttributeName>>,
        entity_type_names: Arc<dyn NameLookup<EntityTypeName>>,
        consequence_publisher: Arc<dyn CommitConsequencePublisher>,
    ) -> PostgresAtomicCommitter {
        PostgresAtomicCommitter {
            pool: pool.clone(), // clone: SQLx pools are shared reference-counted handles.
            history_mode,
            core_write_history,
            attribute_names,
            entity_type_names,
            consequence_publisher,
        }
    }

    /// Apply every current-state mutation and preserve submission-order
    /// outcomes.
    async fn apply_entities(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        writes: &[AtomicWrite],
    ) -> Result<EntityMutations, AttemptFailure> {
        let EntityGroups {
            inserts,
            updates,
            deletes,
            untouched,
        } = group(writes);
        let mut outcomes: Vec<Option<WriteOutcome>> = writes.iter().map(|_| None).collect();
        for &index in &untouched {
            outcomes[index] = Some(WriteOutcome::NoEntityChange);
        }
        // Minted before the fused statements run: both bind the key arrays.
        let names = resolve_write_names(tx, writes, self.entity_type_names.as_ref(), self.attribute_names.as_ref()).await?;
        self.apply_inserts(tx, target, &inserts, &names, &mut outcomes).await?;
        self.apply_updates(tx, target, &updates, &names, &mut outcomes).await?;
        self.apply_deletes(tx, target, &deletes, &mut outcomes).await?;
        let outcomes = outcomes
            .into_iter()
            .enumerate()
            .map(|(index, outcome)| outcome.ok_or(CommitError::Invariant(CommitInvariant::MissingOutcome { index })))
            .collect::<Result<Vec<WriteOutcome>, CommitError>>()?;
        Ok(EntityMutations { outcomes })
    }

    /// Insert the grouped entity rows with one fused statement.
    async fn apply_inserts(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        inserts: &[IndexedWrite<'_>],
        names: &NameKeys,
        outcomes: &mut [Option<WriteOutcome>],
    ) -> Result<(), AttemptFailure> {
        if inserts.is_empty() {
            return Ok(());
        }
        let binds = columns_for(inserts, names)?;
        let inserted = EntityRepository::create_batch_in_tx(tx, target, &binds.entities, binds.rows)
            .await
            .map_err(|error| entity_statement(error))?;
        let landed: HashSet<Identifier> = inserted.into_iter().collect();
        for (index, write) in inserts {
            outcomes[*index] = Some(insert_outcome(write.precondition, returned_flag(&landed, write.entity_id.as_ref())));
        }
        Ok(())
    }

    /// Update the grouped entity rows with one fused statement.
    async fn apply_updates(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        updates: &[IndexedWrite<'_>],
        names: &NameKeys,
        outcomes: &mut [Option<WriteOutcome>],
    ) -> Result<(), AttemptFailure> {
        if updates.is_empty() {
            return Ok(());
        }
        let binds = columns_for(updates, names)?;
        let expected = expectations(updates);
        let saved = EntityRepository::save_batch_in_tx(tx, target, &binds.entities, binds.rows, binds.grains, &expected)
            .await
            .map_err(|error| entity_statement(error))?;
        let landed: HashSet<Identifier> = saved.into_iter().collect();
        for (index, write) in updates {
            outcomes[*index] = Some(update_outcome(write.precondition, returned_flag(&landed, write.entity_id.as_ref())));
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
        let ids: Vec<Identifier> = deletes
            .iter()
            .map(|(_, write)| write.entity_id.as_ref().clone()) // clone: the SQL bind list owns identifiers borrowed from writes.
            .collect();
        let expected = expectations(deletes);
        let mut snapshots: HashMap<Identifier, DeletedEntity> = EntityRepository::delete_batch_in_tx(tx, target, &ids, &expected)
            .await
            .map_err(|error| context_store(error))?
            .into_iter()
            .map(|entity| (entity.id.clone(), entity)) // clone: both the lookup key and returned snapshot own the identifier.
            .collect();
        for (index, write) in deletes {
            outcomes[*index] = Some(delete_outcome(write.precondition, snapshots.remove(write.entity_id.as_ref())));
        }
        Ok(())
    }
}

/// Whether a fused statement returned an entity identifier.
fn returned_flag(landed: &HashSet<Identifier>, id: &Identifier) -> RowReturned {
    if landed.contains(id) { RowReturned::Yes } else { RowReturned::No }
}

impl PostgresAtomicCommitter {
    /// Commit every write under one write identity, numbering every temporal,
    /// notification and side-effect consequence from `base`.
    ///
    /// Every durable consequence of this commit — every landed entity's
    /// temporal history entries, its notification events, and its side-effect
    /// job — shares the one write identity and ingestion instant carried by
    /// `record`, and draws its ordinals from the one running counter that
    /// starts at `base`, in the order [`number_consequences`] gives them.
    /// `commit` and `commit_enqueued` mint a fresh record and start that
    /// counter at zero; `commit_replayed` reuses a durably stored record and resumes
    /// the counter at its stored `base_ordinal`, so a replay's consequences
    /// belong to the write that deferred them and follow the ones that write
    /// already committed.
    ///
    /// One call is one transaction. A transaction the datastore rolls back as
    /// a deadlock or serialization victim ends in
    /// [`CommitError::TransactionContended`] having left nothing behind, which
    /// is what lets [`rerun_while_contended`] run the identical call again
    /// under the same record and base. A write whose own content was refused
    /// ends the attempt in [`AttemptFailure::Refused`], having left nothing
    /// behind either, which is what lets `commit` set that write aside and run
    /// the rest under the same record and base.
    ///
    /// A replay also removes the job it `settles` in that one transaction, as
    /// the transaction's first statement and under the job's lease (see
    /// [`settle`]): a replay whose lease had already lapsed ends in
    /// [`CommitError::ReplayLeaseLost`] before it wrote anything, and one that
    /// held it keeps the job row-locked until it commits, however long the
    /// replay runs.
    ///
    /// `response` says whether the client waits for the consequences, which
    /// decides where a live-branch commit's history is placed.
    async fn commit_under(
        &self,
        target: &ContextTarget,
        record: WriteRecord,
        base: WriteOrdinal,
        writes: &[AtomicWrite],
        settles: Option<JobLease>,
        response: CommitResponse,
    ) -> Result<Vec<WriteOutcome>, AttemptFailure> {
        if writes.is_empty() && settles.is_none() {
            return Ok(Vec::new());
        }
        validate_submission(writes)?;
        let mut tx = self.pool.begin().await.map_err(|error| database(&error))?;
        // Every fallible phase below borrows this owned transaction. SQLx
        // rolls it back when an early `?` drops it, so no entity mutation
        // can survive a failed history, notification or side-effect write,
        // and a settled job is restored with its lease.
        if let Some(lease) = settles {
            settle(&mut tx, lease).await?;
        }
        if writes.is_empty() {
            tx.commit().await.map_err(|error| database(&error))?;
            return Ok(Vec::new());
        }
        let EntityMutations { outcomes } = self.apply_entities(&mut tx, target, writes).await?;
        let landed: Vec<&AtomicWrite> = writes
            .iter()
            .zip(&outcomes)
            .filter(|(_, outcome)| consequences_land(outcome))
            .map(|(write, _)| write)
            .collect();
        let NumberedConsequences {
            intents,
            events,
            events_base,
            job_base,
        } = number_consequences(&landed, &record, base, self.core_write_history)?;
        let placement = self.history_mode.placement(target, response);
        let queued = match placement {
            HistoryPlacement::Inline(recorder) => {
                history::apply(recorder, &mut tx, target, &record, &intents).await?;
                ConsequenceWrite::Nothing
            }
            HistoryPlacement::Queued { recorder, .. } => deferred_history::apply(&mut tx, recorder, &record, &intents).await?,
            HistoryPlacement::BehindQueued { recorder, .. } => behind_queue::apply(&mut tx, recorder, &record, intents).await?,
        };
        let notifications = notification_events::apply(
            &mut tx,
            target,
            events,
            &record,
            events_base,
            self.entity_type_names.as_ref(),
            self.attribute_names.as_ref(),
        )
        .await?;
        let side_effects = side_effects::apply(
            &mut tx,
            target,
            &landed,
            &record,
            job_base,
            self.entity_type_names.as_ref(),
            self.attribute_names.as_ref(),
        )
        .await?;

        tx.commit().await.map_err(|error| database(&error))?;
        tracing::debug!(write_id = %record.write_id(), writes = writes.len(), "atomic context write committed");
        if queued == ConsequenceWrite::Written
            && let HistoryPlacement::Queued { publisher, .. } | HistoryPlacement::BehindQueued { publisher, .. } = placement
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

    /// Commit `writes` under a write identity freshly minted at `accepted_at`,
    /// answered as `response`.
    ///
    /// Every refused write is set aside and the rest run again, one attempt per
    /// refusal, so a successful commit costs one transaction and each refused
    /// entity costs its chunk one more.
    async fn commit_fresh(
        &self,
        target: &ContextTarget,
        accepted_at: SystemInstant,
        writes: Vec<AtomicWrite>,
        response: CommitResponse,
    ) -> Result<Vec<WriteOutcome>, CommitError> {
        let record = WriteRecord::mint(accepted_at.get());
        let mut submission = IsolatedSubmission::new(writes);
        loop {
            match rerun_while_contended(|| self.commit_under(target, record, WriteOrdinal::at(0), submission.writes(), None, response)).await {
                Ok(landed) => return submission.outcomes(landed),
                Err(AttemptFailure::Refused(refused)) => submission.set_aside(&refused.entity_id, refused.refusal)?,
                Err(AttemptFailure::Failed(error)) => return Err(error),
            }
        }
    }
}

impl ContextCommitter for PostgresAtomicCommitter {
    fn commit(&self, target: &ContextTarget, accepted_at: SystemInstant, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        let target = target.clone();
        Box::pin(async move { self.commit_fresh(&target, accepted_at, writes, CommitResponse::Synchronous).await })
    }

    fn commit_enqueued(&self, accepted_at: SystemInstant, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        Box::pin(async move { self.commit_fresh(&ContextTarget::Live, accepted_at, writes, CommitResponse::RespondAsync).await })
    }

    fn commit_replayed(&self, target: &ContextTarget, replay: CommitReplay, writes: Vec<AtomicWrite>) -> CommitFuture<'_> {
        let target = target.clone();
        Box::pin(async move {
            rerun_while_contended(|| {
                self.commit_under(
                    &target,
                    replay.record,
                    replay.base_ordinal,
                    &writes,
                    Some(replay.lease),
                    CommitResponse::Synchronous,
                )
            })
            .await
            .map_err(AttemptFailure::into_commit_error)
        })
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{committer::returned_flag, entity_group::RowReturned};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use std::collections::HashSet;

    #[test]
    fn returned_flag_reports_identifier_membership() {
        let present = "urn:ngsi-ld:A".parse::<Identifier>().expect("valid entity URI");
        let absent = "urn:ngsi-ld:B".parse::<Identifier>().expect("valid entity URI");
        let landed = HashSet::from([present.clone()]); // clone: the set and the probe each own the probe identifier
        assert!(matches!(returned_flag(&landed, &present), RowReturned::Yes));
        assert!(matches!(returned_flag(&landed, &absent), RowReturned::No));
    }
}
