//! What one commit attempt continues: a fresh write, or the replay of a job an
//! earlier commit queued.

use andromeda_context_commit::{commit_replay::CommitReplay, job_lease::JobLease, write_ordinal::WriteOrdinal, write_record::WriteRecord};
use andromeda_postgresql_context_store::notification_delivery::event_commit::EventCommit;

/// The identity, numbering and origin one commit attempt runs under.
#[derive(Clone, Copy, Debug)]
pub(crate) enum CommitContinuation {
    /// A write committed for the first time under a freshly minted record,
    /// its numbering starting at zero.
    Fresh(WriteRecord),
    /// The replay of a job an earlier commit queued.
    Replay(CommitReplay),
}

impl CommitContinuation {
    /// The identity and ingestion instant every consequence shares.
    pub(crate) const fn record(self) -> WriteRecord {
        match self {
            CommitContinuation::Fresh(record) => record,
            CommitContinuation::Replay(replay) => replay.record,
        }
    }

    /// The first ordinal the attempt's consequences draw.
    pub(crate) const fn base(self) -> WriteOrdinal {
        match self {
            CommitContinuation::Fresh(_) => WriteOrdinal::at(0),
            CommitContinuation::Replay(replay) => replay.base_ordinal,
        }
    }

    /// The job the attempt removes under its lease, for a replay.
    pub(crate) const fn settles(self) -> Option<JobLease> {
        match self {
            CommitContinuation::Fresh(_) => None,
            CommitContinuation::Replay(replay) => Some(replay.lease),
        }
    }

    /// The commit whose instant the attempt's notification events record: its
    /// own transaction for a fresh write, the commit that queued the job for a
    /// replay, which made the changes the replay notifies.
    pub(crate) const fn event_commit(self) -> EventCommit {
        match self {
            CommitContinuation::Fresh(_) => EventCommit::ThisTransaction,
            CommitContinuation::Replay(replay) => EventCommit::Earlier(replay.committed_at),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::commit_continuation::CommitContinuation;
    use andromeda_context_commit::{
        commit_replay::CommitReplay,
        job_lease::JobLease,
        lease_holder::LeaseHolder,
        side_effect_job_id::SideEffectJobId,
        store_instant::StoreInstant,
        write_id::WriteId,
        write_ordinal::WriteOrdinal,
        write_record::WriteRecord,
    };
    use andromeda_postgresql_context_store::notification_delivery::event_commit::EventCommit;
    use chrono::{TimeZone, Utc};

    #[test]
    fn a_fresh_commit_numbers_from_zero_settles_nothing_and_stamps_its_own_transaction() {
        let record = WriteRecord::mint(Utc::now());
        let fresh = CommitContinuation::Fresh(record);
        pretty_assertions::assert_eq!(fresh.record(), record);
        pretty_assertions::assert_eq!(fresh.base(), WriteOrdinal::at(0));
        pretty_assertions::assert_eq!(fresh.settles(), None);
        pretty_assertions::assert_eq!(fresh.event_commit(), EventCommit::ThisTransaction);
    }

    #[test]
    fn a_replay_resumes_its_job_and_stamps_the_commit_that_queued_it() {
        let queued_at = StoreInstant::new(Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant"));
        let lease = JobLease {
            job: SideEffectJobId::mint(),
            holder: LeaseHolder::mint(),
        };
        let record = WriteRecord::restored(WriteId::mint(), queued_at.get());
        let replay = CommitContinuation::Replay(CommitReplay::new(record, WriteOrdinal::at(3), lease, queued_at));
        pretty_assertions::assert_eq!(replay.record(), record);
        pretty_assertions::assert_eq!(replay.base(), WriteOrdinal::at(3));
        pretty_assertions::assert_eq!(replay.settles(), Some(lease));
        pretty_assertions::assert_eq!(replay.event_commit(), EventCommit::Earlier(queued_at));
    }
}
