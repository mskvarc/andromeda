//! A walk through a schedule for a failure streak held in memory.

use crate::{retry_number::RetryNumber, schedule::BackoffSchedule};
use std::time::Duration;

/// The position of one in-memory failure streak in its schedule.
///
/// The cursor never runs out: past the ceiling it keeps returning the
/// ceiling's delay until the streak is [`reset`](Self::reset).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackoffCursor {
    /// The schedule the streak follows.
    schedule: BackoffSchedule,
    /// The retry the next failure is followed by.
    next: RetryNumber,
}

impl BackoffCursor {
    /// A cursor at the start of `schedule`.
    #[must_use]
    pub const fn new(schedule: BackoffSchedule) -> BackoffCursor {
        BackoffCursor {
            schedule,
            next: RetryNumber::FIRST,
        }
    }

    /// The delay before the next retry, after which the cursor steps past it.
    pub fn advance(&mut self) -> Duration {
        let delay = self.schedule.delay(self.next);
        self.next = self.next.next();
        delay
    }

    /// End the streak, so the next delay is the schedule's first.
    pub const fn reset(&mut self) {
        self.next = RetryNumber::FIRST;
    }

    /// The schedule the cursor walks.
    #[must_use]
    pub const fn schedule(&self) -> &BackoffSchedule {
        &self.schedule
    }
}

#[cfg(test)]
mod tests {
    use crate::{cursor::BackoffCursor, growth::Growth, schedule::BackoffSchedule};
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    fn cursor() -> BackoffCursor {
        BackoffCursor::new(BackoffSchedule::new(Duration::from_secs(1), Duration::from_secs(8), Growth::Doubling))
    }

    #[test]
    fn advancing_walks_the_schedule_and_holds_its_ceiling() {
        let mut cursor = cursor();
        let delays: Vec<_> = (0..5).map(|_| cursor.advance()).collect();
        assert_eq!(delays, [1, 2, 4, 8, 8].map(Duration::from_secs));
    }

    #[test]
    fn a_reset_restarts_at_the_first_delay() {
        let mut cursor = cursor();
        cursor.advance();
        cursor.advance();
        cursor.reset();
        assert_eq!(cursor.advance(), Duration::from_secs(1));
    }
}
