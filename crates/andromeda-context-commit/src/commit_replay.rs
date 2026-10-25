//! The record and ordinal base a replayed commit continues under, and the job
//! lease it settles.

use crate::{job_lease::JobLease, write_ordinal::WriteOrdinal, write_record::WriteRecord};

/// The identity and next free ordinal a durably enqueued side-effect job must
/// resume its commit under.
///
/// A batch side-effect job enqueued by one commit is later leased and
/// re-submitted through
/// [`ContextCommitter::commit_replayed`](crate::committer::ContextCommitter::commit_replayed).
/// Reusing the original [`WriteRecord`] keeps every row the replay produces
/// under the identity the original commit's other consequences already share,
/// so the deferred history and notification events belong to the write that
/// caused them; resuming the ordinal counter at `base_ordinal` rather than at
/// zero numbers them after the consequences the original commit already
/// wrote, never over one of them.
///
/// The replay also removes the job it continues, in the same transaction and
/// only while `lease` is still held, before it writes anything: a replay whose
/// lease was lost commits nothing, and one that removed its job holds it until
/// it commits. Only one replay can remove the job and every other one rolls
/// back with its writes, so a job's consequences land exactly once, with its
/// removal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommitReplay {
    /// Identity and ingestion instant every consequence of the original commit shares.
    pub record: WriteRecord,
    /// First ordinal not yet used by the original commit's own consequences.
    pub base_ordinal: WriteOrdinal,
    /// The job the replay settles and the lease it must still hold to do so.
    pub lease: JobLease,
}

impl CommitReplay {
    /// Construct a replay continuation from its record, next free ordinal and
    /// the lease of the job it settles.
    #[must_use]
    pub const fn new(record: WriteRecord, base_ordinal: WriteOrdinal, lease: JobLease) -> CommitReplay {
        CommitReplay { record, base_ordinal, lease }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        commit_replay::CommitReplay,
        job_lease::JobLease,
        lease_holder::LeaseHolder,
        side_effect_job_id::SideEffectJobId,
        write_id::WriteId,
        write_ordinal::WriteOrdinal,
        write_record::WriteRecord,
    };
    use chrono::{TimeZone, Utc};

    #[test]
    fn a_replay_carries_the_record_base_and_lease_it_was_built_from() {
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let record = WriteRecord::restored(WriteId::mint(), recorded_at);
        let lease = JobLease {
            job: SideEffectJobId::mint(),
            holder: LeaseHolder::mint(),
        };
        let replay = CommitReplay::new(record, WriteOrdinal::at(4), lease);
        pretty_assertions::assert_eq!(replay.record, record);
        pretty_assertions::assert_eq!(replay.base_ordinal, WriteOrdinal::at(4));
        pretty_assertions::assert_eq!(replay.lease, lease);
    }
}
