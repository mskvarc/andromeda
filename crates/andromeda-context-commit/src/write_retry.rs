//! How often, and how far apart, a lost compare-and-set write is retried.
//!
//! A read-modify-write states the `modifiedAt` it computed its payload from, so
//! any writer that commits in between takes the attempt down with it. With `n`
//! writers on one entity, the entity a writer loses to is itself retrying, and
//! attempts that restart in lockstep collide again for the same reason they
//! collided the first time. Convergence therefore needs both a budget large
//! enough that a writer can lose to every peer ahead of it and a wait that
//! spreads the restarts apart, which is why the two live in one policy.

use std::time::Duration;

/// Number of read-modify-write attempts made before reporting contention.
///
/// A writer converges once it has outlived every peer that was ahead of it, so
/// the budget is what bounds the concurrency on a single entity the broker
/// absorbs rather than reports. It is deliberately far above the number of
/// writers a single client produces: an attempt that loses costs one read and
/// one refused write, and paying that a dozen times is cheaper for every party
/// than a failed request the client has to compose again.
pub const WRITE_ATTEMPTS: u32 = 12;

/// Ceiling on the wait after a single lost attempt, in microseconds.
///
/// The first wait is deliberately shorter than a round trip to the store: two
/// writers that collide once need to be separated, not delayed.
const FIRST_BACKOFF_MICROS: u64 = 2_000;

/// Hard ceiling on any single wait, in microseconds.
///
/// Doubling stops here so that a request spends its budget inside a latency a
/// client still recognises as a write rather than a timeout.
const MAX_BACKOFF_MICROS: u64 = 50_000;

/// The longest wait the policy can ask for after `lost_attempts` lost attempts.
///
/// Doubles from [`FIRST_BACKOFF_MICROS`] and saturates at
/// [`MAX_BACKOFF_MICROS`]; `0` lost attempts yields no wait at all.
#[must_use]
pub const fn backoff_ceiling(lost_attempts: u32) -> Duration {
    Duration::from_micros(backoff_ceiling_micros(lost_attempts))
}

/// The wait to observe before the attempt that follows `lost_attempts` losses.
///
/// The wait is drawn uniformly from the whole interval up to
/// [`backoff_ceiling`] — full jitter rather than a fixed delay, because a fixed
/// delay moves colliding writers together instead of separating them.
#[must_use]
pub fn backoff_after_lost_attempts(lost_attempts: u32) -> Duration {
    Duration::from_micros(fastrand::u64(0..=backoff_ceiling_micros(lost_attempts)))
}

/// The doubled, saturated ceiling in the unit the jitter is drawn in.
const fn backoff_ceiling_micros(lost_attempts: u32) -> u64 {
    if lost_attempts == 0 {
        return 0;
    }
    let doublings = lost_attempts - 1;
    // Above this many doublings the shift alone would overflow, and the result
    // has long since saturated at the ceiling anyway.
    if doublings >= u64::BITS {
        return MAX_BACKOFF_MICROS;
    }
    let uncapped = FIRST_BACKOFF_MICROS.saturating_mul(1_u64 << doublings);
    if uncapped < MAX_BACKOFF_MICROS { uncapped } else { MAX_BACKOFF_MICROS }
}

#[cfg(test)]
mod tests {
    use crate::write_retry::{FIRST_BACKOFF_MICROS, MAX_BACKOFF_MICROS, WRITE_ATTEMPTS, backoff_after_lost_attempts, backoff_ceiling};
    use std::time::Duration;

    #[test]
    fn the_budget_absorbs_more_writers_than_a_single_client_produces() {
        const { assert!(WRITE_ATTEMPTS >= 8, "the budget cannot outlive eight concurrent writers") }
    }

    #[test]
    fn nothing_is_waited_before_the_first_attempt() {
        pretty_assertions::assert_eq!(backoff_ceiling(0), Duration::ZERO);
        pretty_assertions::assert_eq!(backoff_after_lost_attempts(0), Duration::ZERO);
    }

    #[test]
    fn the_ceiling_doubles_from_the_first_wait_until_it_saturates() {
        pretty_assertions::assert_eq!(backoff_ceiling(1), Duration::from_micros(FIRST_BACKOFF_MICROS));
        pretty_assertions::assert_eq!(backoff_ceiling(2), Duration::from_micros(FIRST_BACKOFF_MICROS * 2));
        pretty_assertions::assert_eq!(backoff_ceiling(3), Duration::from_micros(FIRST_BACKOFF_MICROS * 4));
        pretty_assertions::assert_eq!(backoff_ceiling(WRITE_ATTEMPTS), Duration::from_micros(MAX_BACKOFF_MICROS));
    }

    #[test]
    fn the_ceiling_never_exceeds_its_bound_however_many_attempts_are_lost() {
        for lost in [WRITE_ATTEMPTS, WRITE_ATTEMPTS * 4, u32::MAX] {
            assert!(
                backoff_ceiling(lost) <= Duration::from_micros(MAX_BACKOFF_MICROS),
                "the ceiling escaped its bound after {lost} lost attempts"
            );
        }
    }

    #[test]
    fn the_ceiling_never_decreases_as_attempts_are_lost() {
        for lost in 1..=WRITE_ATTEMPTS {
            assert!(
                backoff_ceiling(lost) >= backoff_ceiling(lost - 1),
                "the ceiling shrank between {} and {lost} lost attempts",
                lost - 1
            );
        }
    }

    #[test]
    fn every_drawn_wait_stays_within_the_ceiling_of_its_attempt() {
        for lost in 1..=WRITE_ATTEMPTS {
            for _ in 0..64 {
                assert!(
                    backoff_after_lost_attempts(lost) <= backoff_ceiling(lost),
                    "a drawn wait exceeded the ceiling after {lost} lost attempts"
                );
            }
        }
    }

    #[test]
    fn a_whole_spent_budget_stays_inside_a_second() {
        let worst_case: Duration = (1..WRITE_ATTEMPTS).map(backoff_ceiling).sum();
        assert!(worst_case < Duration::from_secs(1), "a spent budget waits {worst_case:?}");
    }
}
