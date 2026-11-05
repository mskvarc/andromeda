//! Native PostgreSQL atomic context commits.

use crate::{
    attempt_failure::AttemptFailure,
    behind_queue,
    columns::{columns_for, expectations},
    commit_continuation::CommitContinuation,
    commit_refusal::{context_store, database, entity_statement},
    consequences::{
        ConsequenceWrite,
        notification_events,
        numbering::{NumberedConsequences, number_consequences},
        side_effects,
    },
    contention_retry::rerun_while_contended,
    deferred_history,
    entity_group::{EntityGroups, IndexedWrite, delete_outcome, group, insert_outcome, returned_rows, take_returned, update_outcome},
    evolution_establishment,
    history,
    history_mode::{CommitResponse, HistoryPlacement, TemporalHistoryCommitMode},
    history_only_lock,
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
    error::CommitError,
    write_outcome::WriteOutcome,
    write_record::WriteRecord,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_postgresql_attribute_instance::name_keys::NameKeys;
use andromeda_postgresql_context_store::entity::{
    created_entities::CreatedEntities,
    removed_entity::RemovedEntity,
    repo::EntityRepository,
    written_entities::WrittenEntities,
};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use std::{collections::HashSet, sync::Arc};

/// What one commit's entity statements produced.
struct EntityMutations {
    /// Submission-order outcome of every write in the commit.
    outcomes: Vec<WriteOutcome>,
    /// The created Entities whose Temporal Evolution the create established.
    established: Vec<Identifier>,
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
    /// outcomes. Every entity inserted or updated is stamped `accepted_at`,
    /// the instant its attributes carry (clause 4.8), and a created entity in
    /// `establishing` has its Temporal Evolution established beside it. A
    /// write to an Entity holding a later instant lands nothing, and its
    /// outcome carries that instant.
    async fn apply_entities(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        writes: &[AtomicWrite],
        accepted_at: DateTime<Utc>,
        establishing: &HashSet<Identifier>,
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
        let CreatedEntities { landed, established, predated } = self.apply_inserts(tx, target, &inserts, &names, accepted_at, establishing).await?;
        let mut returned = returned_rows(WrittenEntities { landed, predated }, |entity| (entity.id, entity.committed_at));
        for (index, write) in &inserts {
            outcomes[*index] = Some(insert_outcome(write.precondition, take_returned(&mut returned, write.entity_id.as_ref())));
        }
        self.apply_updates(tx, target, &updates, &names, accepted_at, &mut outcomes).await?;
        self.apply_deletes(tx, target, &deletes, accepted_at, &mut outcomes).await?;
        let outcomes = outcomes
            .into_iter()
            .enumerate()
            .map(|(index, outcome)| outcome.ok_or(CommitError::Invariant(CommitInvariant::MissingOutcome { index })))
            .collect::<Result<Vec<WriteOutcome>, CommitError>>()?;
        Ok(EntityMutations { outcomes, established })
    }

    /// Insert the grouped entity rows with one fused statement, returning
    /// what it landed.
    async fn apply_inserts(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        inserts: &[IndexedWrite<'_>],
        names: &NameKeys,
        accepted_at: DateTime<Utc>,
        establishing: &HashSet<Identifier>,
    ) -> Result<CreatedEntities, AttemptFailure> {
        if inserts.is_empty() {
            return Ok(CreatedEntities::default());
        }
        let binds = columns_for(inserts, names)?;
        EntityRepository::create_batch_in_tx(tx, target, &binds.entities, binds.rows, accepted_at, establishing)
            .await
            .map_err(|error| entity_statement(error))
    }

    /// Update the grouped entity rows with one fused statement.
    async fn apply_updates(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        updates: &[IndexedWrite<'_>],
        names: &NameKeys,
        accepted_at: DateTime<Utc>,
        outcomes: &mut [Option<WriteOutcome>],
    ) -> Result<(), AttemptFailure> {
        if updates.is_empty() {
            return Ok(());
        }
        let binds = columns_for(updates, names)?;
        let expected = expectations(updates);
        let saved = EntityRepository::save_batch_in_tx(tx, target, &binds.entities, binds.rows, binds.grains, &expected, accepted_at)
            .await
            .map_err(|error| entity_statement(error))?;
        let mut returned = returned_rows(saved, |entity| (entity.id, entity.committed_at));
        for (index, write) in updates {
            outcomes[*index] = Some(update_outcome(write.precondition, take_returned(&mut returned, write.entity_id.as_ref())));
        }
        Ok(())
    }

    /// Delete the grouped entity rows stamped `accepted_at` and retain their
    /// pre-deletion state.
    async fn apply_deletes(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        deletes: &[IndexedWrite<'_>],
        accepted_at: DateTime<Utc>,
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
        let removed = EntityRepository::delete_batch_in_tx(tx, target, &ids, &expected, accepted_at)
            .await
            .map_err(|error| context_store(error))?;
        let mut returned = returned_rows(removed, |removed: RemovedEntity| {
            (removed.entity.id.clone(), (removed.entity, removed.committed_at)) // clone: both the lookup key and returned snapshot own the identifier.
        });
        for (index, write) in deletes {
            outcomes[*index] = Some(delete_outcome(write.precondition, take_returned(&mut returned, write.entity_id.as_ref())));
        }
        Ok(())
    }
}

impl PostgresAtomicCommitter {
    /// Commit every write under the write identity `continuation` carries,
    /// numbering every temporal, notification and side-effect consequence from
    /// its base.
    ///
    /// Every durable consequence of this commit — every landed entity's
    /// temporal history entries, its notification events, and its side-effect
    /// job — shares the one write identity and ingestion instant carried by
    /// the record, and draws its ordinals from the one running counter that
    /// starts at the base, in the order [`number_consequences`] gives them.
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
    /// A replay also removes the job it settles in that one transaction, as
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
        continuation: CommitContinuation,
        writes: &[AtomicWrite],
        response: CommitResponse,
    ) -> Result<Vec<WriteOutcome>, AttemptFailure> {
        let record = continuation.record();
        let base = continuation.base();
        let settles = continuation.settles();
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
        history_only_lock::apply(&mut tx, target, writes).await?;
        let placement = self.history_mode.placement(target, response);
        let establishing = evolution_establishment::establishing(placement.inline_recording(), writes, self.core_write_history);
        let EntityMutations { outcomes, established } = self.apply_entities(&mut tx, target, writes, record.recorded_at(), &establishing).await?;
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
        let queued = match placement {
            HistoryPlacement::Inline(recorder) => {
                let established = evolution_establishment::confirm(&mut tx, target, &intents, established).await?;
                history::apply(recorder, &mut tx, target, &record, &intents, &established).await?;
                ConsequenceWrite::Nothing
            }
            HistoryPlacement::Queued { recorder, .. } => {
                evolution_establishment::confirm(&mut tx, target, &[], established).await?;
                deferred_history::apply(&mut tx, recorder, &record, &intents).await?
            }
            HistoryPlacement::BehindQueued { recorder, .. } => behind_queue::apply(&mut tx, recorder, &record, intents, established).await?,
        };
        let notifications = notification_events::apply(
            &mut tx,
            target,
            events,
            continuation,
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
            match rerun_while_contended(|| self.commit_under(target, CommitContinuation::Fresh(record), submission.writes(), response)).await {
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
            rerun_while_contended(|| self.commit_under(&target, CommitContinuation::Replay(replay), &writes, CommitResponse::Synchronous))
                .await
                .map_err(AttemptFailure::into_commit_error)
        })
    }
}
