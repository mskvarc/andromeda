//! The record and ordinal base a replayed commit continues under.

use crate::{write_ordinal::WriteOrdinal, write_record::WriteRecord};

/// The identity and next free ordinal a durably enqueued side-effect job must
/// resume its commit under.
///
/// A batch side-effect job enqueued by one commit is later leased and
/// re-submitted through
/// [`ContextCommitter::commit_replayed`](crate::committer::ContextCommitter::commit_replayed).
/// Reusing the original [`WriteRecord`] keeps every row the replay produces
/// under the identity the original commit's other consequences already share;
/// resuming the ordinal counter at `base_ordinal` rather than at zero is
/// what keeps a replayed history entry or notification event from colliding
/// with an ordinal the original commit already spent on one of its own
/// consequences. Together the two are what makes a crash-retried delivery of
/// the same job idempotent: a second attempt reproduces exactly the write
/// identity and ordinals the first attempt would have produced, so its rows
/// collide on the row-level `ON CONFLICT ... DO NOTHING` constraints instead
/// of duplicating anything.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommitReplay {
    /// Identity and ingestion instant every consequence of the original commit shares.
    pub record: WriteRecord,
    /// First ordinal not yet used by the original commit's own consequences.
    pub base_ordinal: WriteOrdinal,
}

impl CommitReplay {
    /// Construct a replay continuation from its record and next free ordinal.
    #[must_use]
    pub const fn new(record: WriteRecord, base_ordinal: WriteOrdinal) -> CommitReplay {
        CommitReplay { record, base_ordinal }
    }
}

#[cfg(test)]
mod tests {
    use crate::{commit_replay::CommitReplay, write_id::WriteId, write_ordinal::WriteOrdinal, write_record::WriteRecord};
    use chrono::{TimeZone, Utc};

    #[test]
    fn a_replay_carries_the_record_and_base_it_was_built_from() {
        let recorded_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("valid instant");
        let record = WriteRecord::restored(WriteId::mint(), recorded_at);
        let replay = CommitReplay::new(record, WriteOrdinal::at(4));
        pretty_assertions::assert_eq!(replay.record, record);
        pretty_assertions::assert_eq!(replay.base_ordinal, WriteOrdinal::at(4));
    }
}
