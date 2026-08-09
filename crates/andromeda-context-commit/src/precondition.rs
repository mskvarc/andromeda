//! Entity mutation preconditions and retry classification.

use crate::write_outcome::WriteOutcome;
use chrono::{DateTime, Utc};
use derive_more::IsVariant;

/// Assumption an atomic write makes about current entity state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, IsVariant)]
pub enum Precondition {
    /// Apply without an existence or version condition.
    #[default]
    None,
    /// The entity must not exist.
    MustNotExist,
    /// The entity must exist.
    MustExist,
    /// The entity must retain the observed modification timestamp.
    UnchangedSince(DateTime<Utc>),
}

impl Precondition {
    /// Return the compare-and-set timestamp, when this is a version condition.
    pub const fn expected_modified_at(self) -> Option<DateTime<Utc>> {
        match self {
            Precondition::UnchangedSince(modified_at) => Some(modified_at),
            Precondition::None | Precondition::MustNotExist | Precondition::MustExist => None,
        }
    }
}

/// Number of read-modify-write attempts made before reporting contention.
pub const WRITE_ATTEMPTS: u32 = 3;

/// Next action after one read-modify-write commit attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterAttempt {
    /// The requested mutation or independent consequence settled.
    Settled,
    /// Current state changed and must be read again.
    Reread,
    /// The target entity vanished.
    Vanished,
}

/// Classify a write outcome for the read-modify-write retry loop.
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
    use crate::{
        precondition::{AfterAttempt, WRITE_ATTEMPTS, after_attempt},
        write_outcome::WriteOutcome,
    };

    #[test]
    fn a_superseded_write_requests_a_reread() {
        pretty_assertions::assert_eq!(after_attempt(&WriteOutcome::Superseded), AfterAttempt::Reread);
    }

    #[test]
    fn a_missing_entity_stops_the_retry_loop() {
        pretty_assertions::assert_eq!(after_attempt(&WriteOutcome::NotFound), AfterAttempt::Vanished);
    }

    #[test]
    fn a_landed_write_settles() {
        for outcome in [
            WriteOutcome::Created,
            WriteOutcome::Updated,
            WriteOutcome::NoEntityChange,
            WriteOutcome::Skipped,
        ] {
            pretty_assertions::assert_eq!(after_attempt(&outcome), AfterAttempt::Settled);
        }
    }

    #[test]
    fn the_retry_budget_is_three_attempts() {
        pretty_assertions::assert_eq!(WRITE_ATTEMPTS, 3);
    }
}
