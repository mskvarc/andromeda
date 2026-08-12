//! The position of one consequence inside its atomic write.

use derive_more::Display;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Zero-based position of one durable consequence within its atomic write.
///
/// Together with [`WriteId`](crate::write_id::WriteId) it is the deduplicating
/// identity of a persisted row: a replay of the same write reproduces the same
/// ordinals for the same consequences, so the second attempt collides with the
/// rows the first attempt wrote instead of duplicating them. The ordering is
/// therefore required to be deterministic, which is why it is derived from the
/// position in the write's ordered consequence list and never from arrival
/// order at the persistence layer.
#[derive(Clone, Copy, Debug, Deserialize, Display, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[display("{_0}")]
pub struct WriteOrdinal(u32);

impl WriteOrdinal {
    /// Return the ordinal of a consequence at `position` in its write's list.
    pub const fn at(position: u32) -> WriteOrdinal {
        WriteOrdinal(position)
    }

    /// Return the in-memory representation.
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Return the persistence representation.
    pub const fn as_i32(self) -> Result<i32, WriteOrdinalOutOfRange> {
        if self.0 <= i32::MAX as u32 {
            Ok(self.0 as i32)
        } else {
            Err(WriteOrdinalOutOfRange)
        }
    }

    /// Return the ordinal `delta` positions after this one.
    ///
    /// A commit draws every consequence's ordinal from one running counter that
    /// starts at zero (or, for a replay, at the durably stored
    /// [`CommitReplay::consequence_base`](crate::commit_replay::CommitReplay))
    /// and advances by the number of consequences each phase consumes. Checked
    /// arithmetic is what keeps a pathological write from wrapping the counter
    /// back into a range an earlier consequence already claimed.
    pub const fn advanced_by(self, delta: u32) -> Result<WriteOrdinal, WriteOrdinalOutOfRange> {
        match self.0.checked_add(delta) {
            Some(sum) => Ok(WriteOrdinal(sum)),
            None => Err(WriteOrdinalOutOfRange),
        }
    }
}

impl TryFrom<usize> for WriteOrdinal {
    type Error = WriteOrdinalOutOfRange;

    fn try_from(position: usize) -> Result<WriteOrdinal, WriteOrdinalOutOfRange> {
        u32::try_from(position).map(WriteOrdinal).map_err(|_| WriteOrdinalOutOfRange)
    }
}

impl TryFrom<i32> for WriteOrdinal {
    type Error = WriteOrdinalOutOfRange;

    fn try_from(stored: i32) -> Result<WriteOrdinal, WriteOrdinalOutOfRange> {
        u32::try_from(stored).map(WriteOrdinal).map_err(|_| WriteOrdinalOutOfRange)
    }
}

/// A write ordinal did not fit the range shared by memory and storage.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("write ordinal is outside the range representable in both memory and storage")]
pub struct WriteOrdinalOutOfRange;

#[cfg(test)]
mod tests {
    use crate::write_ordinal::{WriteOrdinal, WriteOrdinalOutOfRange};

    #[test]
    fn ordinals_follow_consequence_positions() {
        let ordinals: Vec<WriteOrdinal> = (0..4_usize)
            .map(|position| WriteOrdinal::try_from(position).expect("small positions are representable"))
            .collect();
        assert_eq!(
            ordinals,
            vec![WriteOrdinal::at(0), WriteOrdinal::at(1), WriteOrdinal::at(2), WriteOrdinal::at(3)]
        );
    }

    #[test]
    fn ordinals_sort_by_position() {
        let mut ordinals = vec![WriteOrdinal::at(2), WriteOrdinal::at(0), WriteOrdinal::at(1)];
        ordinals.sort_unstable();
        assert_eq!(ordinals, vec![WriteOrdinal::at(0), WriteOrdinal::at(1), WriteOrdinal::at(2)]);
    }

    #[test]
    fn a_stored_ordinal_round_trips_through_persistence() {
        let ordinal = WriteOrdinal::at(7);
        let stored = ordinal.as_i32().expect("small ordinals are representable");
        assert_eq!(WriteOrdinal::try_from(stored), Ok(ordinal));
    }

    #[test]
    fn an_ordinal_beyond_the_storage_range_is_refused() {
        assert_eq!(WriteOrdinal::at(u32::MAX).as_i32(), Err(WriteOrdinalOutOfRange));
    }

    #[test]
    fn a_negative_stored_ordinal_is_refused() {
        assert_eq!(WriteOrdinal::try_from(-1_i32), Err(WriteOrdinalOutOfRange));
    }

    #[test]
    fn advancing_an_ordinal_adds_the_delta_to_its_position() {
        assert_eq!(WriteOrdinal::at(5).advanced_by(3), Ok(WriteOrdinal::at(8)));
    }

    #[test]
    fn advancing_by_zero_returns_the_same_ordinal() {
        assert_eq!(WriteOrdinal::at(5).advanced_by(0), Ok(WriteOrdinal::at(5)));
    }

    #[test]
    fn advancing_an_ordinal_past_u32_max_is_refused_rather_than_wrapped() {
        assert_eq!(WriteOrdinal::at(u32::MAX).advanced_by(1), Err(WriteOrdinalOutOfRange));
    }

    #[test]
    fn advancing_from_zero_by_u32_max_reaches_the_maximum_ordinal() {
        assert_eq!(WriteOrdinal::at(0).advanced_by(u32::MAX), Ok(WriteOrdinal::at(u32::MAX)));
    }
}
