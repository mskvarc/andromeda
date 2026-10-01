//! Committing one chunk of a batch as a read-modify-write of each entity in it.
//!
//! Clause 5.5.11.0 has a batch treat its entities "as if they had all arrived
//! in separate requests", and clauses 5.6.8.4, 5.6.9.4, 5.6.10.4 and 5.6.20.4
//! run the single-entity behaviour for each of them. Each entity's write is
//! computed from a state read outside the writing transaction, so it states the
//! clause 4.8 `modifiedAt` of that state, exactly as the single-entity
//! operation does (see [`crate::read_modify_write`]). The store refuses a write
//! whose entity moved on since, and an insert of an entity that appeared since;
//! only those entities are read again and rebuilt. A concurrent writer --
//! another broker replica, or a Create Entity, which takes no per-identifier
//! guard -- is therefore never overwritten, and no entity is answered as
//! processed when its write did not land.
//!
//! The preconditions are stated here and not by the paths, so a path cannot
//! leave one out: a write built against a snapshot holds while the entity is
//! unchanged since that snapshot, and a write built for an absent entity holds
//! while the entity is still absent.
//!
//! Every attempt stamps the one write instant the chunk took before the first,
//! because "All Entities and Attributes in the batch will get the same
//! modifiedAt timestamp" (clause 5.5.11.0).

use crate::{
    EntityService,
    batch::chunk::{ChunkOutcome, ChunkWrite},
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    side_effect::SideEffectPlacement,
};
use andromeda_context_commit::{precondition::Precondition, write_retry::WRITE_ATTEMPTS};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use chrono::{DateTime, Utc};
use std::{collections::HashSet, future::Future};

/// One batch path's part in a chunk read-modify-write: what it reads besides
/// the entities' snapshots, and the write it builds for one entity against the
/// state an attempt read.
pub(super) trait ChunkRewrite: Sync {
    /// What the path builds one entity's write from. It is handed back with
    /// every write, because a write the store refuses is rebuilt from it.
    type Entry: Send;
    /// What one attempt reads for the pending entities besides their snapshots.
    type Reads: Default + Send + Sync;

    /// The entity `entry` writes.
    fn id(entry: &Self::Entry) -> &Identifier;

    /// Read what the path needs besides the snapshots of the `pending`
    /// entities. Called after the snapshots are read, so whatever it reads is
    /// no older than they are.
    fn read(&self, pending: &[Identifier]) -> impl Future<Output = Result<Self::Reads, EntityError>> + Send {
        let _ = pending;
        async { Ok(Self::Reads::default()) }
    }

    /// Build the write of `entry` against `snapshot`, the entity as this
    /// attempt read it, or `None` when the attempt found it absent.
    ///
    /// The write's precondition is stated by the driver, whatever the build
    /// set.
    ///
    /// # Errors
    ///
    /// Answers the entity's identifier with the failure that settles it
    /// without a write, boxed because a failure is the rare outcome.
    fn build(
        &self,
        entry: Self::Entry,
        snapshot: Option<&EntitySnapshot>,
        reads: &Self::Reads,
        now: DateTime<Utc>,
    ) -> Result<PlannedWrite<Self::Entry>, Box<(Identifier, EntityError)>>;
}

/// The write one attempt built for one entity.
pub(super) struct PlannedWrite<E> {
    /// What the write is rebuilt from if the store refuses it.
    pub entry: E,
    /// The write to commit.
    pub write: ChunkWrite,
    /// A failure reported beside the write once it settles, such as an append
    /// that added nothing. It describes this attempt only, so a refused
    /// attempt's is dropped with it.
    pub alongside: Option<EntityError>,
}

impl EntityService {
    /// Commit every entity of `entries` through `rewrite`, re-reading and
    /// rebuilding whatever a concurrent writer got to first.
    ///
    /// The per-identifier guards are held across every attempt, so a writer in
    /// this process waits rather than burning attempts; the compare-and-set is
    /// what keeps a writer this process does not own from being overwritten.
    ///
    /// # Errors
    ///
    /// Reports a chunk-level read or commit failure. An entity that loses its
    /// precondition on every attempt is reported in [`ChunkOutcome::errors`]
    /// as [`EntityError::Contention`].
    pub(super) async fn commit_chunk_read_modify_write<R: ChunkRewrite>(
        &self,
        placement: SideEffectPlacement<'_>,
        rewrite: &R,
        entries: Vec<R::Entry>,
    ) -> Result<ChunkOutcome, EntityError> {
        let mut settled = ChunkOutcome::default();
        if entries.is_empty() {
            return Ok(settled);
        }
        let chunk_ids: Vec<Identifier> = entries.iter().map(|entry| R::id(entry).clone()).collect(); // clone: the guards and the first read own the identifiers
        let _id_guards = self.acquire_id_locks(placement.target(), &chunk_ids).await;
        let now = Utc::now();

        let mut pending = self.rewrite_attempt(placement, rewrite, entries, &chunk_ids, now, &mut settled).await?;
        for lost_attempts in 1..WRITE_ATTEMPTS {
            if pending.is_empty() {
                return Ok(settled);
            }
            self.wait_before_retry(lost_attempts).await;
            let pending_ids: Vec<Identifier> = pending.iter().map(|entry| R::id(entry).clone()).collect(); // clone: the re-read owns the identifiers
            pending = self.rewrite_attempt(placement, rewrite, pending, &pending_ids, now, &mut settled).await?;
        }
        settled.errors.extend(pending.iter().map(|entry| {
            let id = R::id(entry);
            (id.clone(), EntityError::contention(id, WRITE_ATTEMPTS)) // clone: the report owns the identifier it names
        }));
        Ok(settled)
    }

    /// One attempt over the `pending` entities, whose identifiers are `ids`:
    /// read them, build and commit their writes, fold whatever settled into
    /// `settled`, and hand back the entries whose write the store refused
    /// because the entity moved on.
    async fn rewrite_attempt<R: ChunkRewrite>(
        &self,
        placement: SideEffectPlacement<'_>,
        rewrite: &R,
        pending: Vec<R::Entry>,
        ids: &[Identifier],
        now: DateTime<Utc>,
        settled: &mut ChunkOutcome,
    ) -> Result<Vec<R::Entry>, EntityError> {
        let mut snapshots = self.reader.get_snapshot_batch(placement.target(), ids).await?;
        let reads = rewrite.read(ids).await?;

        let mut writes = Vec::with_capacity(pending.len());
        let mut built = Vec::with_capacity(pending.len());
        for entry in pending {
            let snapshot = snapshots.remove(R::id(&entry));
            match rewrite.build(entry, snapshot.as_ref(), &reads, now) {
                Ok(PlannedWrite { entry, mut write, alongside }) => {
                    write.precondition = snapshot.map_or(Precondition::MustNotExist, |snapshot| Precondition::UnchangedSince(snapshot.modified_at));
                    writes.push(write);
                    built.push((entry, alongside));
                }
                Err(failure) => settled.errors.push(*failure),
            }
        }

        let outcome = self.commit_chunk(placement, writes).await?;
        let ChunkOutcome {
            success,
            created,
            errors,
            retryable,
        } = outcome;
        settled.success.extend(success);
        settled.created.extend(created);
        settled.errors.extend(errors);

        let refused: HashSet<&Identifier> = retryable.iter().map(|(id, _)| id).collect();
        let mut next = Vec::with_capacity(refused.len());
        for (entry, alongside) in built {
            if refused.contains(R::id(&entry)) {
                next.push(entry);
            } else if let Some(alongside) = alongside {
                settled.errors.push((R::id(&entry).clone(), alongside)); // clone: the report owns the identifier it names
            }
        }
        Ok(next)
    }
}
