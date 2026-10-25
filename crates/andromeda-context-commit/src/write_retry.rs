//! How often, and how far apart, a lost compare-and-set write is retried.
//!
//! A read-modify-write states the `modifiedAt` it computed its payload from, so
//! any writer that commits in between takes the attempt down with it. With `n`
//! writers on one entity, the entity a writer loses to is itself retrying, and
//! attempts that restart in lockstep collide again for the same reason they
//! collided the first time. Convergence therefore needs both a budget large
//! enough that a writer can lose to every peer ahead of it and a wait that
//! spreads the restarts apart, which is why the two live in one policy.

use andromeda_retry::{growth::Growth, jitter::Jitter, retry_policy::RetryPolicy, schedule::BackoffSchedule};
use std::{num::NonZeroU32, time::Duration};

/// Number of read-modify-write attempts made before reporting contention.
///
/// A writer converges once it has outlived every peer that was ahead of it, so
/// the budget is what bounds the concurrency on a single entity the broker
/// absorbs rather than reports. It is deliberately far above the number of
/// writers a single client produces: an attempt that loses costs one read and
/// one refused write, and paying that a dozen times is cheaper for every party
/// than a failed request the client has to compose again.
pub const WRITE_ATTEMPTS: u32 = 12;

/// The wait after each lost attempt.
///
/// The first ceiling (2 ms) is deliberately shorter than a round trip to the
/// store: two writers that collide once need to be separated, not delayed.
/// Doubling stops at 50 ms so that a request spends its budget inside a
/// latency a client still recognises as a write rather than a timeout. The
/// wait is drawn from the whole interval up to the ceiling (full jitter),
/// because a fixed delay moves colliding writers together instead of
/// separating them.
pub const WRITE_BACKOFF: BackoffSchedule =
    BackoffSchedule::new(Duration::from_millis(2), Duration::from_millis(50), Growth::Doubling).with_jitter(Jitter::Full);

/// The budget and the wait a contended read-modify-write follows.
pub const WRITE_RETRY: RetryPolicy = RetryPolicy {
    attempts: NonZeroU32::MIN.saturating_add(WRITE_ATTEMPTS - 1),
    schedule: WRITE_BACKOFF,
};

#[cfg(test)]
mod tests {
    use crate::write_retry::{WRITE_ATTEMPTS, WRITE_BACKOFF, WRITE_RETRY};
    use andromeda_retry::retry_number::RetryNumber;
    use std::{num::NonZeroU32, time::Duration};

    fn retry(ordinal: u32) -> RetryNumber {
        RetryNumber::new(NonZeroU32::new(ordinal).unwrap_or(NonZeroU32::MIN))
    }

    #[test]
    fn the_budget_absorbs_more_writers_than_a_single_client_produces() {
        const { assert!(WRITE_ATTEMPTS >= 8, "the budget cannot outlive eight concurrent writers") }
    }

    #[test]
    fn the_retry_policy_runs_the_whole_budget() {
        pretty_assertions::assert_eq!(WRITE_RETRY.attempts.get(), WRITE_ATTEMPTS);
    }

    #[test]
    fn the_ceiling_doubles_from_the_first_wait_until_it_saturates() {
        let ceilings: Vec<_> = [1, 2, 3, WRITE_ATTEMPTS]
            .into_iter()
            .map(|ordinal| WRITE_BACKOFF.ceiling(retry(ordinal)))
            .collect();
        pretty_assertions::assert_eq!(ceilings, [2, 4, 8, 50].map(Duration::from_millis));
    }

    #[test]
    fn the_ceiling_never_decreases_as_attempts_are_lost() {
        for ordinal in 2..=WRITE_ATTEMPTS {
            assert!(
                WRITE_BACKOFF.ceiling(retry(ordinal)) >= WRITE_BACKOFF.ceiling(retry(ordinal - 1)),
                "the ceiling shrank before retry {ordinal}"
            );
        }
    }

    #[test]
    fn every_drawn_wait_stays_within_the_ceiling_of_its_attempt() {
        for ordinal in 1..=WRITE_ATTEMPTS {
            for _ in 0..64 {
                assert!(
                    WRITE_BACKOFF.delay(retry(ordinal)) <= WRITE_BACKOFF.ceiling(retry(ordinal)),
                    "a drawn wait exceeded the ceiling of retry {ordinal}"
                );
            }
        }
    }

    #[test]
    fn a_whole_spent_budget_stays_inside_a_second() {
        let worst_case: Duration = (1..WRITE_ATTEMPTS).map(|ordinal| WRITE_BACKOFF.ceiling(retry(ordinal))).sum();
        assert!(worst_case < Duration::from_secs(1), "a spent budget waits {worst_case:?}");
    }
}
