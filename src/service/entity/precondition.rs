//! Optimistic concurrency control for the entity write path.
//!
//! A write that was computed from something the caller read states what it
//! assumed about that state, and the store refuses the write when the
//! assumption no longer holds. That replaces reading inside the writing
//! transaction, which is the only shape a store whose history or outbox lives
//! elsewhere cannot implement.

use crate::service::entity::write_outcome::WriteOutcome;
use chrono::{DateTime, Utc};
use derive_more::IsVariant;

/// What an [`AtomicWrite`](crate::service::entity::atomic_write::AtomicWrite)
/// assumes about the entity it is about to write.
///
/// `UnchangedSince` carries the entity's clause 4.8 `modifiedAt` as it stood
/// when the caller read it. PostgreSQL compares it against the `modified_at`
/// column, which is set from `NOW()` — transaction start time, at microsecond
/// resolution. Two writes to one row cannot overlap, because the second waits
/// on the first's row lock, so the two timestamps can only collide if two
/// sequential transactions on the same row began inside the same microsecond
/// with a network round trip between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IsVariant)]
pub enum Precondition {
    /// Write regardless of what is there.
    None,
    /// Write only if no entity with this identifier exists.
    MustNotExist,
    /// Write only if an entity with this identifier exists.
    MustExist,
    /// Write only if the entity has not been modified since this instant.
    UnchangedSince(DateTime<Utc>),
}

impl Precondition {
    /// The `modifiedAt` this precondition expects to find, if it expects one.
    pub const fn expected_modified_at(&self) -> Option<DateTime<Utc>> {
        match self {
            Precondition::UnchangedSince(modified_at) => Some(*modified_at),
            Precondition::None | Precondition::MustNotExist | Precondition::MustExist => None,
        }
    }
}

/// How many times a read-modify-write is attempted before it gives up.
///
/// The per-id guards in [`acquire_id_locks_in`](crate::service::entity::id_lock::acquire_id_locks_in)
/// serialise same-identifier writers inside this process, so a superseded write
/// means a second broker process or an out-of-band writer touched the row
/// inside the one-round-trip window between the read and the commit. Three
/// attempts converge on that with overwhelming probability while bounding the
/// worst case at two extra round trips.
pub const WRITE_ATTEMPTS: u32 = 3;

/// What a read-modify-write does after one attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IsVariant)]
pub enum AfterAttempt {
    /// The write landed, or declined to land for a reason a re-read cannot fix.
    Settled,
    /// Another writer moved the entity on; read it again and recompute.
    Reread,
    /// The entity is not there, and no number of attempts will change that.
    Vanished,
}

/// Classify one attempt's outcome.
///
/// `AlreadyExists` asks for a re-read rather than settling: the caller that
/// meets it read the identifier as absent a moment ago, and the re-read turns
/// the write into an update that tombstones what the replacement drops
/// (NGSI-LD 5.6.8).
pub const fn after_attempt(outcome: &WriteOutcome) -> AfterAttempt {
    match outcome {
        WriteOutcome::Created | WriteOutcome::Updated | WriteOutcome::Deleted(_) | WriteOutcome::NoEntityChange | WriteOutcome::Skipped => {
            AfterAttempt::Settled
        }
        WriteOutcome::Superseded | WriteOutcome::AlreadyExists => AfterAttempt::Reread,
        WriteOutcome::NotFound => AfterAttempt::Vanished,
    }
}

#[cfg(test)]
mod tests {
    use super::{AfterAttempt, WRITE_ATTEMPTS, after_attempt};
    use crate::service::entity::{store::DeletedEntity, write_outcome::WriteOutcome};
    use chrono::Utc;
    use serde_json::json;

    fn deleted() -> WriteOutcome {
        let now = Utc::now();
        WriteOutcome::Deleted(DeletedEntity {
            id: "urn:ngsi-ld:A".to_string(),
            payload: json!([{}]),
            created_at: now,
            modified_at: now,
            expires_at: None,
        })
    }

    #[test]
    fn a_superseded_write_asks_for_a_re_read() {
        pretty_assertions::assert_eq!(after_attempt(&WriteOutcome::Superseded), AfterAttempt::Reread);
    }

    #[test]
    fn a_landed_write_settles() {
        for outcome in [
            WriteOutcome::Created,
            WriteOutcome::Updated,
            deleted(),
            WriteOutcome::NoEntityChange,
            WriteOutcome::Skipped,
        ] {
            pretty_assertions::assert_eq!(after_attempt(&outcome), AfterAttempt::Settled);
        }
    }

    #[test]
    fn an_absent_entity_stops_the_loop() {
        pretty_assertions::assert_eq!(after_attempt(&WriteOutcome::NotFound), AfterAttempt::Vanished);
    }

    #[test]
    fn a_conflicting_insert_asks_for_a_re_read() {
        pretty_assertions::assert_eq!(after_attempt(&WriteOutcome::AlreadyExists), AfterAttempt::Reread);
    }

    #[test]
    fn the_attempt_budget_is_three() {
        pretty_assertions::assert_eq!(WRITE_ATTEMPTS, 3);
    }
}
