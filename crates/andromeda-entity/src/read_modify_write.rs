//! Turning a read plus a computation into an optimistic write.
//!
//! Every entity mutation that is a function of the entity's current payload —
//! merge, replace, all five attribute-level operations, the clause 5.6.21 purge
//! and the clause 4.22 sweep — reads the payload outside the writing
//! transaction. The write it produces is therefore only meaningful against the
//! payload it was computed from, so it states that payload's clause 4.8
//! `modifiedAt` and the store refuses it if the entity has moved on.
//!
//! Without that, a concurrent writer's change is silently overwritten: two
//! requests that read the same payload and append different attributes each
//! write the whole entity back, and the second erases the first while both of
//! their history rows land. The Entity API and the Temporal Evolution then
//! disagree permanently, because nothing sweeps the difference away.

use crate::{EntityService, error::EntityError};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    precondition::{AfterAttempt, Precondition, WRITE_ATTEMPTS, after_attempt},
    write_outcome::{WriteOutcome, single},
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use serde_json::Value;

impl EntityService {
    /// Commit one entity's read-modify-write, re-reading whatever a concurrent
    /// writer superseded.
    ///
    /// `build` turns the entity's current payload into the write that
    /// supersedes it, or `None` when the payload needs no write at all. It runs
    /// once per attempt, so it must not consume anything it needs again.
    ///
    /// Returns `None` when the entity does not exist or when `build` declined to
    /// produce a write.
    ///
    /// # Errors
    ///
    /// Propagates whatever `build` or the coordinator returns, and reports
    /// [`EntityError::internal`] once the attempt budget is spent.
    pub(crate) async fn commit_read_modify_write<B>(&self, id: &Uri, mut build: B) -> Result<Option<WriteOutcome>, EntityError>
    where
        B: FnMut(&Value) -> Result<Option<AtomicWrite>, EntityError>,
    {
        for attempt in 1..=WRITE_ATTEMPTS {
            let Some(snapshot) = self.reader.get_snapshot(id).await? else {
                return Ok(None);
            };
            let Some(mut write) = build(&snapshot.payload)? else {
                return Ok(None);
            };
            write.precondition = Precondition::UnchangedSince(snapshot.modified_at);

            let outcome = single(self.writes.commit(vec![write]).await?)?;
            match after_attempt(&outcome) {
                AfterAttempt::Settled => return Ok(Some(outcome)),
                AfterAttempt::Vanished => return Ok(None),
                AfterAttempt::Reread => {
                    if attempt == WRITE_ATTEMPTS {
                        return Err(exhausted(id));
                    }
                }
            }
        }
        Err(exhausted(id))
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
    /// # Errors
    ///
    /// Reports [`EntityError::NotFound`] when the entity is absent,
    /// and otherwise propagates whatever `build` or the coordinator returns.
    pub(crate) async fn commit_computed_write<B, T>(&self, id: &Uri, mut build: B) -> Result<T, EntityError>
    where
        B: FnMut(&Value) -> Result<(AtomicWrite, T), EntityError>,
    {
        let mut produced: Option<T> = None;
        let landed = self
            .commit_read_modify_write(id, |previous| {
                let (write, value) = build(previous)?;
                produced = Some(value);
                Ok(Some(write))
            })
            .await?;

        if landed.is_none() {
            return Err(EntityError::not_found(id));
        }
        produced.ok_or_else(|| EntityError::internal(format!("read-modify-write for {id} landed without producing a result")))
    }
}

/// The error a read-modify-write reports once it has spent its attempts.
///
/// NGSI-LD 5.5.2 has no ProblemDetails type for "the broker lost a
/// compare-and-set", and answering 409 would tell the client to retry a request
/// the broker has already retried [`WRITE_ATTEMPTS`] times.
fn exhausted(id: &Uri) -> EntityError {
    EntityError::internal(format!("entity {id} was modified by another writer on every one of {WRITE_ATTEMPTS} attempts"))
}
