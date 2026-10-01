//! Removing the side-effect job a replayed commit settles.
//!
//! A replay removes its job and commits the job's remaining consequences in one
//! transaction, under the job's lease. Two transactions would leave a window in
//! which the consequences had landed but the job was still queued: a lease lost
//! in that window lets another worker reclaim and replay the job, and a
//! live-delivery broker would then publish its changes a second time.
//!
//! The removal is the transaction's first statement, not its last. The
//! lease-guarded `DELETE` either misses -- the lease lapsed and the job may
//! already be another worker's -- and the replay is refused before it wrote
//! anything, or it hits and holds the job's row lock until the transaction
//! ends. While that lock is held the claim's `FOR UPDATE SKIP LOCKED` scan
//! steps over the row, so a replay slower than its lease can no longer have
//! the job reclaimed underneath it: the lease only has to cover the wait
//! between the claim and the start of the replay, never the replay itself.
//!
//! That ordering is also what makes a job's consequences land exactly once.
//! Exactly one replay transaction of a job can commit, because only one can
//! delete its row; every replay that does not commit rolls the removal back
//! together with whatever it wrote. The consequences it commits are written
//! under the job's stored write identity, numbered from its stored ordinal
//! base, so they follow the ones the enqueueing commit wrote under that
//! identity rather than overlapping them.

use crate::commit_refusal::context_store;
use andromeda_context_commit::{error::CommitError, job_lease::JobLease, lease_acknowledgement::LeaseAcknowledgement};
use andromeda_postgresql_context_store::pending_side_effects::repo::PendingSideEffectsRepository;
use sqlx::{Postgres, Transaction};

/// Remove the job `lease` names as the first statement of the replay's
/// transaction, refusing the whole replay when the lease was lost.
///
/// Called before any consequence is written, so a refusal leaves the
/// transaction empty: a worker whose lease lapsed before its replay started
/// writes nothing, and the reclaiming worker's replay is the one that lands. A
/// removal that succeeds keeps the row locked until the transaction commits or
/// rolls back; a rollback restores the job under this worker's lease, to be
/// released or reclaimed like any other failed attempt.
///
/// # Errors
///
/// Returns [`CommitError::ReplayLeaseLost`] when the job is no longer leased to
/// `lease.holder`, and the classified store failure when the statement fails.
pub(crate) async fn settle(tx: &mut Transaction<'_, Postgres>, lease: JobLease) -> Result<(), CommitError> {
    match PendingSideEffectsRepository::settle_in_tx(tx, lease)
        .await
        .map_err(|error| context_store(error))?
    {
        LeaseAcknowledgement::Applied => Ok(()),
        LeaseAcknowledgement::Lost => Err(CommitError::ReplayLeaseLost { job: lease.job }),
    }
}
