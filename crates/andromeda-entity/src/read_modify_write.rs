//! Turning a read plus a computation into an optimistic write.
//!
//! Every entity mutation that is a function of the entity's current state —
//! merge, replace, all five attribute-level operations, the clause 5.6.21 purge
//! and the clause 4.22 sweep — reads the state outside the writing
//! transaction. The write it produces is therefore only meaningful against the
//! state it was computed from, so it states that state's clause 4.8
//! `modifiedAt` and the store refuses it if the entity has moved on.
//!
//! Without that, a concurrent writer's change is silently overwritten: two
//! requests that read the same payload and append different attributes each
//! write the whole entity back, and the second erases the first while both of
//! their history rows land. The Entity API and the Temporal Evolution then
//! disagree permanently, because nothing sweeps the difference away.

use crate::{EntityService, entity_invariant::EntityInvariant, error::EntityError};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    precondition::{AfterAttempt, Precondition, after_attempt},
    write_outcome::{WriteOutcome, single},
    write_retry::{WRITE_ATTEMPTS, backoff_after_lost_attempts},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use std::{slice::from_ref, sync::Arc};

/// What one attempt of a computed write asked the store to do.
#[derive(Clone, Copy)]
enum ComputedWrite {
    /// The attempt produced a write to store.
    Stored,
    /// The attempt changed nothing, so nothing is stored.
    Nothing,
}

impl ComputedWrite {
    /// What an attempt that produced `write` asked of the store.
    const fn of(write: Option<&AtomicWrite>) -> ComputedWrite {
        match write {
            Some(_) => ComputedWrite::Stored,
            None => ComputedWrite::Nothing,
        }
    }
}

impl EntityService {
    /// Commit one entity's read-modify-write, re-reading whatever a concurrent
    /// writer superseded.
    ///
    /// `build` turns the entity's current state into the write that
    /// supersedes it, or `None` when the state needs no write at all. It runs
    /// once per attempt, so it must not consume anything it needs again.
    ///
    /// Returns `None` when the entity does not exist or when `build` declined to
    /// produce a write.
    ///
    /// Requests that target the same entity take the same broker-side guard, so
    /// the compare-and-set only ever loses to a writer this process does not
    /// own — another broker instance, or a batch chunk holding the guard for a
    /// set of identifiers. Those losses are absorbed by re-reading behind the
    /// [`backoff_after_lost_attempts`] wait, which separates writers that
    /// collided rather than restarting them together.
    ///
    /// # Errors
    ///
    /// Propagates whatever `build` or the coordinator returns, and reports
    /// [`EntityError::Contention`] once the attempt budget is spent.
    pub(crate) async fn commit_read_modify_write<B>(&self, target: &ContextTarget, id: &Identifier, mut build: B) -> Result<Option<WriteOutcome>, EntityError>
    where
        B: FnMut(&Arc<EntityState>) -> Result<Option<AtomicWrite>, EntityError>,
    {
        let _id_guard = self.acquire_id_locks(target, from_ref(id)).await;

        for lost_attempts in 0..WRITE_ATTEMPTS {
            self.wait_before_retry(lost_attempts).await;

            let Some(snapshot) = self.reader.get_snapshot(target, id).await? else {
                return Ok(None);
            };
            let Some(mut write) = build(&snapshot.state)? else {
                return Ok(None);
            };
            write.precondition = Precondition::UnchangedSince(snapshot.modified_at);

            let outcome = single(self.writes.commit(target, vec![write]).await?)?;
            match after_attempt(&outcome) {
                AfterAttempt::Settled => return Ok(Some(outcome)),
                AfterAttempt::Vanished => return Ok(None),
                // The attempt lost its precondition: fall through to the next
                // one, which re-reads the state the write is computed from.
                AfterAttempt::Reread => (),
            }
        }
        Err(EntityError::contention(id, WRITE_ATTEMPTS))
    }

    /// Hold a writer that has lost `lost_attempts` attempts before it tries
    /// again; a writer that has lost none is not held at all.
    pub(crate) async fn wait_before_retry(&self, lost_attempts: u32) {
        if lost_attempts > 0 {
            self.retry_delay.wait(backoff_after_lost_attempts(lost_attempts)).await;
        }
    }

    /// Commit a read-modify-write and hand back whatever its computation
    /// produced besides the write.
    ///
    /// The notification a mutation raises is a function of the same merge that
    /// produced the write — which attributes it dropped, what the entity looked
    /// like before it, what it looks like after — and that merge runs again on
    /// every attempt. Reading those values off the attempt that actually landed
    /// is what keeps the notification describing the change the store accepted
    /// rather than one an earlier attempt computed and lost.
    ///
    /// `build` returns no write when the computation changed nothing (clause
    /// 5.4: an empty Fragment is a JSON Merge Patch that returns its target
    /// unchanged), and then nothing is stored: the entity's `modifiedAt` stays
    /// as it was. The value `build` produced is still handed back.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::NotFound`] when the entity is absent,
    /// and otherwise propagates whatever `build` or the coordinator returns.
    pub(crate) async fn commit_computed_write<B, T>(&self, target: &ContextTarget, id: &Identifier, mut build: B) -> Result<T, EntityError>
    where
        B: FnMut(&Arc<EntityState>) -> Result<(Option<AtomicWrite>, T), EntityError>,
    {
        let mut produced: Option<(T, ComputedWrite)> = None;
        let landed = self
            .commit_read_modify_write(target, id, |previous| {
                let (write, value) = build(previous)?;
                produced = Some((value, ComputedWrite::of(write.as_ref())));
                Ok(write)
            })
            .await?;

        match (landed, produced) {
            (Some(_), Some((value, ComputedWrite::Stored | ComputedWrite::Nothing))) | (None, Some((value, ComputedWrite::Nothing))) => Ok(value),
            (None, Some((_, ComputedWrite::Stored)) | None) => Err(EntityError::not_found(id)),
            (Some(_), None) => Err(EntityInvariant::ReadModifyWriteWithoutResult {
                entity: id.clone(), // clone: the error owns the entity it names.
            }
            .into()),
        }
    }
}
