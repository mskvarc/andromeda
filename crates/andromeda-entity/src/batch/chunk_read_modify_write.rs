//! Committing one chunk of a batch as a read-modify-write of each entity in it.
//!
//! Clause 5.5.11.0 has a batch treat its entities "as if they had all arrived
//! in separate requests", and clauses 5.6.8.4, 5.6.9.4, 5.6.10.4 and 5.6.20.4
//! run the single-entity behaviour for each of them. Each entity's write is
//! computed from a state read outside the writing transaction, so it states the
//! clause 4.8 `modifiedAt` of that state, exactly as the single-entity
//! operation does (see [`crate::read_modify_write`]). The store refuses a write
//! whose entity moved on since, and an insert of an entity that appeared since;
//! only those entities are read again and rebuilt. A concurrent writer
//! (another broker replica, or a Create Entity, which takes no per-identifier
//! guard) is therefore never overwritten, and no entity is answered as
//! processed when its write did not land.
//!
//! The preconditions are stated here and not by the paths, so a path cannot
//! leave one out: a write built against a snapshot holds while the entity is
//! unchanged since that snapshot, and a write built for an absent entity holds
//! while the entity is still absent.
//!
//! The first attempt stamps the one write instant the batch took before its
//! first round, because "All Entities and Attributes in the batch will get the
//! same modifiedAt timestamp" (clause 5.5.11.0). The store also refuses a write
//! to an entity holding a later instant than the batch's, one another writer
//! stamped while the batch ran: written at the batch instant, the entity's
//! `modifiedAt` would move backwards (clause 4.8). Such an entity, like one
//! another writer superseded, is written as a separate request would write it,
//! so every later attempt takes its own instant: the clock's, or one
//! microsecond past the latest instant a refusal answered
//! ([`AttemptInstant`]). Every entity no other write touched keeps the batch
//! instant.

use crate::{
    EntityService,
    batch::chunk::{ChunkOutcome, ChunkWrite, GuardedWrite},
    contended_attempt::ContendedAttempt,
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    side_effect::SideEffectPlacement,
};
use andromeda_context_commit::{attempt_instant::AttemptInstant, precondition::Precondition, write_retry::WRITE_ATTEMPTS};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_retry::retry_policy::RetryAttempt;
use std::{
    collections::{HashMap, HashSet},
    mem::take,
};

/// What a batch path reads before it builds its writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChunkRead {
    /// Every attempt reads the entities it writes, and a refusal a fresh read
    /// can fix is attempted again.
    Snapshots,
    /// No attempt reads: every write creates its entity (clause 5.6.7), so an
    /// entity that exists is a conflict no read can fix, and only a write the
    /// store refused as predated is attempted again.
    Nothing,
}

/// One batch path's part in a chunk read-modify-write: the write it builds for
/// one entity against the entity as an attempt read it.
pub(super) trait ChunkRewrite: Sync {
    /// What the path builds one entity's write from. It is handed back with
    /// every write, because a write the store refuses is rebuilt from it.
    type Entry: Send;

    /// What the path reads before it builds its writes.
    const READ: ChunkRead;

    /// The entity `entry` writes.
    fn id(entry: &Self::Entry) -> &Identifier;

    /// Build the write of `entry` against `snapshot`, the entity as this
    /// attempt read it, or `None` when the attempt found it absent or read
    /// nothing.
    ///
    /// The write's precondition is the driver's to state: the build only
    /// says what to write.
    ///
    /// # Errors
    ///
    /// Answers the entity's identifier with the failure that settles it
    /// without a write, boxed because a failure is the rare outcome.
    fn build(
        &self,
        entry: Self::Entry,
        snapshot: Option<&EntitySnapshot>,
        now: SystemInstant,
    ) -> Result<PlannedWrite<Self::Entry>, Box<(Identifier, EntityError)>>;
}

/// What the attempts of one chunk carry from one to the next.
#[derive(Default)]
struct ChunkProgress {
    /// Every entity an attempt settled.
    settled: ChunkOutcome,
    /// The earliest instant the next attempt may be stamped at.
    instant: AttemptInstant,
}

/// The write one attempt built for one entity.
pub(super) struct PlannedWrite<E> {
    /// What the write is rebuilt from if the store refuses it.
    pub entry: E,
    /// The write to commit, or `None` when the entry changes nothing: the
    /// entity is then settled as a success without a write, so its
    /// `modifiedAt`, subscribers and history stay untouched.
    pub write: Option<ChunkWrite>,
}

impl EntityService {
    /// Commit every entity of `entries` through `rewrite`, first at the batch
    /// instant `now`, re-reading and rebuilding whatever a concurrent writer
    /// got to first.
    ///
    /// The per-identifier guards are held across every attempt, so a writer in
    /// this process waits rather than burning attempts; the compare-and-set is
    /// what keeps a writer this process does not own from being overwritten.
    ///
    /// # Errors
    ///
    /// Reports a chunk-level read or commit failure. An entity whose write is
    /// refused on every attempt is reported in [`ChunkOutcome::errors`] as
    /// [`EntityError::Contention`].
    pub(super) async fn commit_chunk_read_modify_write<R: ChunkRewrite>(
        &self,
        placement: SideEffectPlacement<'_>,
        rewrite: &R,
        entries: Vec<R::Entry>,
        now: SystemInstant,
    ) -> Result<ChunkOutcome, EntityError> {
        let mut progress = ChunkProgress::default();
        if entries.is_empty() {
            return Ok(progress.settled);
        }
        let chunk_ids: Vec<Identifier> = entries.iter().map(|entry| R::id(entry).clone()).collect(); // clone: the guards and the first read own the identifiers
        let _id_guards = self.acquire_id_locks(placement.target(), &chunk_ids).await;

        let mut pending = entries;
        let contended = self
            .retry_contended(async |attempt| {
                let entries = take(&mut pending);
                let remaining = match attempt {
                    RetryAttempt::First => self.rewrite_attempt(placement, rewrite, entries, &chunk_ids, now, &mut progress).await?,
                    RetryAttempt::Retry(_) => {
                        let pending_ids: Vec<Identifier> = entries.iter().map(|entry| R::id(entry).clone()).collect(); // clone: the re-read owns the identifiers
                        let now = progress.instant.next();
                        self.rewrite_attempt(placement, rewrite, entries, &pending_ids, now, &mut progress).await?
                    }
                };
                pending = remaining;
                if pending.is_empty() { Ok(()) } else { Err(ContendedAttempt::Lost) }
            })
            .await;
        let mut settled = progress.settled;
        match contended {
            Ok(()) => Ok(settled),
            Err(ContendedAttempt::Lost) => {
                settled.errors.extend(pending.iter().map(|entry| {
                    let id = R::id(entry);
                    (id.clone(), EntityError::contention(id, WRITE_ATTEMPTS)) // clone: the report owns the identifier it names
                }));
                Ok(settled)
            }
            Err(ContendedAttempt::Failed(error)) => Err(error),
        }
    }

    /// One attempt over the `pending` entities, whose identifiers are `ids`:
    /// read them, build and commit their writes at `now`, fold whatever
    /// settled into `progress` with the latest instant a predated write's
    /// entity holds, and hand back the entries whose write the store refused
    /// because the entity moved on or holds a later instant.
    async fn rewrite_attempt<R: ChunkRewrite>(
        &self,
        placement: SideEffectPlacement<'_>,
        rewrite: &R,
        pending: Vec<R::Entry>,
        ids: &[Identifier],
        now: SystemInstant,
        progress: &mut ChunkProgress,
    ) -> Result<Vec<R::Entry>, EntityError> {
        let ChunkProgress { settled, instant } = progress;
        let mut snapshots = match R::READ {
            ChunkRead::Snapshots => self.reader.get_snapshot_batch(placement.target(), ids).await?,
            ChunkRead::Nothing => HashMap::new(),
        };

        let mut writes = Vec::with_capacity(pending.len());
        let mut built = Vec::with_capacity(pending.len());
        for entry in pending {
            let snapshot = snapshots.remove(R::id(&entry));
            match rewrite.build(entry, snapshot.as_ref(), now) {
                Ok(PlannedWrite { entry, write: Some(write) }) => {
                    let precondition = snapshot.map_or(Precondition::MustNotExist, |snapshot| Precondition::UnchangedSince(snapshot.version));
                    writes.push(GuardedWrite { write, precondition });
                    built.push(entry);
                }
                // The state this attempt read already is what the entry asks
                // for, so a concurrent writer moving the entity on does not
                // make the answer wrong: nothing is stored and nothing retried.
                Ok(PlannedWrite { entry, write: None }) => settled.success.push(R::id(&entry).clone()), // clone: the report owns the identifier it names
                Err(failure) => settled.errors.push(*failure),
            }
        }

        let outcome = self.commit_chunk(placement, now, writes).await?;
        let ChunkOutcome {
            success,
            created,
            errors,
            retryable,
            predated,
        } = outcome;
        settled.success.extend(success);
        settled.created.extend(created);
        settled.errors.extend(errors);

        let mut refused: HashSet<Identifier> = predated
            .into_iter()
            .map(|(id, latest)| {
                instant.refused(latest);
                id
            })
            .collect();
        match R::READ {
            ChunkRead::Snapshots => refused.extend(retryable.into_iter().map(|(id, _)| id)),
            ChunkRead::Nothing => settled.errors.extend(retryable),
        }
        Ok(built.into_iter().filter(|entry| refused.contains(R::id(entry))).collect())
    }
}
