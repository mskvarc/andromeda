//! The outcomes of one or more notifications that one delivery-state write
//! records.
//!
//! Every outcome of one subscription lands on the same delivery-state row, so
//! outcomes that arrive together are recorded by one write carrying their sum:
//! `timesSent` and `timesFailed` advance by the counts, and `lastSuccess`,
//! `lastFailure` and `lastNotification` each move once (clause 5.8.6, Table
//! 5.2.15-1). A tally always holds at least one outcome, so the write it
//! becomes always moves `lastSuccess` or `lastFailure`.

use crate::notification_slot::{NotificationOutcome, OutcomeRecord};
use chrono::{DateTime, Utc};
use std::num::NonZeroU32;

/// The notifications of a tally that are counted with their outcome rather
/// than at their claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CountedNotifications {
    /// How many there are: `timesSent` advances by it.
    pub count: NonZeroU32,
    /// The latest instant any of them was claimed at, on the store's clock:
    /// `lastNotification` moves to it unless a later notification already
    /// moved it further.
    pub latest: DateTime<Utc>,
}

/// The outcomes of one or more notifications of one subscription, added up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutcomeTally {
    /// Notifications the endpoint accepted: `lastSuccess` moves when positive.
    successes: u32,
    /// Notifications the endpoint refused or did not answer: `timesFailed`
    /// advances by it, and `lastFailure` moves when positive.
    failures: u32,
    /// The notifications counted with their outcome, `None` when the claims
    /// counted every one of them or none was sent.
    counted: Option<CountedNotifications>,
}

impl OutcomeTally {
    /// The tally of one notification's `outcome`, with what `record` adds to
    /// it.
    #[must_use]
    pub fn new(outcome: NotificationOutcome, record: OutcomeRecord) -> OutcomeTally {
        let mut tally = OutcomeTally {
            successes: 0,
            failures: 0,
            counted: None,
        };
        tally.include(outcome, record);
        tally
    }

    /// Add one more notification's `outcome`, with what `record` adds to it.
    pub fn include(&mut self, outcome: NotificationOutcome, record: OutcomeRecord) {
        match outcome {
            NotificationOutcome::Success => self.successes = self.successes.saturating_add(1),
            NotificationOutcome::Failure => self.failures = self.failures.saturating_add(1),
        }
        match record {
            OutcomeRecord::OutcomeOnly => {}
            OutcomeRecord::CountingNotification { notified_at } => {
                self.counted = Some(match self.counted {
                    None => CountedNotifications {
                        count: NonZeroU32::MIN,
                        latest: notified_at,
                    },
                    Some(counted) => CountedNotifications {
                        count: counted.count.saturating_add(1),
                        latest: counted.latest.max(notified_at),
                    },
                });
            }
        }
    }

    /// How many of the notifications the endpoint accepted.
    #[must_use]
    pub const fn successes(&self) -> u32 {
        self.successes
    }

    /// How many of the notifications the endpoint refused or did not answer.
    #[must_use]
    pub const fn failures(&self) -> u32 {
        self.failures
    }

    /// The notifications counted with their outcome, `None` when there are
    /// none.
    #[must_use]
    pub const fn counted(&self) -> Option<CountedNotifications> {
        self.counted
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        notification_slot::{NotificationOutcome, OutcomeRecord},
        outcome_tally::{CountedNotifications, OutcomeTally},
    };
    use chrono::{DateTime, TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU32;

    fn instant(seconds: i64) -> Result<DateTime<Utc>, &'static str> {
        Utc.timestamp_opt(seconds, 0).single().ok_or("the fixture timestamp must be valid")
    }

    #[test]
    fn one_success_counted_at_its_claim_tallies_one_success_and_nothing_counted() {
        let tally = OutcomeTally::new(NotificationOutcome::Success, OutcomeRecord::OutcomeOnly);
        assert_eq!((tally.successes(), tally.failures(), tally.counted()), (1, 0, None));
    }

    #[test]
    fn one_failure_counted_with_its_outcome_tallies_one_failure_and_one_notification() -> Result<(), &'static str> {
        let notified_at = instant(100)?;
        let tally = OutcomeTally::new(NotificationOutcome::Failure, OutcomeRecord::CountingNotification { notified_at });
        assert_eq!(
            (tally.successes(), tally.failures(), tally.counted()),
            (
                0,
                1,
                Some(CountedNotifications {
                    count: NonZeroU32::MIN,
                    latest: notified_at
                })
            )
        );
        Ok(())
    }

    #[test]
    fn outcomes_added_out_of_claim_order_keep_the_latest_claim_instant() -> Result<(), &'static str> {
        let mut tally = OutcomeTally::new(NotificationOutcome::Success, OutcomeRecord::CountingNotification { notified_at: instant(200)? });
        tally.include(NotificationOutcome::Failure, OutcomeRecord::CountingNotification { notified_at: instant(300)? });
        tally.include(NotificationOutcome::Success, OutcomeRecord::CountingNotification { notified_at: instant(100)? });
        tally.include(NotificationOutcome::Success, OutcomeRecord::OutcomeOnly);
        let three = NonZeroU32::new(3).ok_or("three is positive")?;
        assert_eq!(
            (tally.successes(), tally.failures(), tally.counted()),
            (
                3,
                1,
                Some(CountedNotifications {
                    count: three,
                    latest: instant(300)?
                })
            )
        );
        Ok(())
    }
}
