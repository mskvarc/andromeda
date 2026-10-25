//! How a delay grows with the retry number before its ceiling stops it.

use crate::retry_number::RetryNumber;
use std::time::Duration;

/// The widest doubling a `u32` multiplier can apply in one step.
const WIDEST_DOUBLING_STEP: u32 = u32::BITS - 1;

/// How the first delay is multiplied at a given retry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Growth {
    /// The delay doubles with every retry: `first * 2^(n-1)`.
    Doubling,
    /// The delay grows by the first delay with every retry: `first * n`.
    Linear,
}

impl Growth {
    /// The un-capped delay at `retry`, saturating at [`Duration::MAX`].
    #[must_use]
    pub const fn grow(self, first: Duration, retry: RetryNumber) -> Duration {
        match self {
            Growth::Doubling => double(first, retry.get() - 1),
            Growth::Linear => first.saturating_mul(retry.get()),
        }
    }
}

/// `delay * 2^doublings`, applied in steps a `u32` multiplier can hold.
///
/// The walk stops as soon as the delay is zero or saturated, which every
/// non-zero delay reaches within a handful of steps, so even the last retry
/// number costs no more than that.
const fn double(delay: Duration, doublings: u32) -> Duration {
    let mut delay = delay;
    let mut remaining = doublings;
    while remaining > 0 && !delay.is_zero() && delay.as_nanos() < Duration::MAX.as_nanos() {
        let step = if remaining < WIDEST_DOUBLING_STEP { remaining } else { WIDEST_DOUBLING_STEP };
        delay = delay.saturating_mul(1 << step);
        remaining -= step;
    }
    delay
}

#[cfg(test)]
mod tests {
    use crate::{growth::Growth, retry_number::RetryNumber};
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroU32, time::Duration};

    fn retry(ordinal: u32) -> RetryNumber {
        RetryNumber::new(NonZeroU32::new(ordinal).unwrap_or(NonZeroU32::MIN))
    }

    #[test]
    fn doubling_starts_at_the_first_delay_and_doubles() {
        let delays: Vec<_> = (1..=4).map(|ordinal| Growth::Doubling.grow(Duration::from_secs(1), retry(ordinal))).collect();
        assert_eq!(delays, [1, 2, 4, 8].map(Duration::from_secs));
    }

    #[test]
    fn doubling_past_the_width_of_a_u32_stays_exact() {
        assert_eq!(Growth::Doubling.grow(Duration::from_nanos(1), retry(40)), Duration::from_nanos(1 << 39));
    }

    #[test]
    fn doubling_saturates_at_the_longest_duration() {
        assert_eq!(Growth::Doubling.grow(Duration::MAX / 2, retry(10)), Duration::MAX);
        assert_eq!(Growth::Doubling.grow(Duration::from_nanos(1), RetryNumber::new(NonZeroU32::MAX)), Duration::MAX);
    }

    #[test]
    fn doubling_a_zero_delay_stays_zero() {
        assert_eq!(Growth::Doubling.grow(Duration::ZERO, RetryNumber::new(NonZeroU32::MAX)), Duration::ZERO);
    }

    #[test]
    fn linear_growth_multiplies_by_the_retry_number() {
        assert_eq!(Growth::Linear.grow(Duration::from_secs(1), retry(5)), Duration::from_secs(5));
        assert_eq!(
            Growth::Linear.grow(Duration::from_nanos(1), RetryNumber::new(NonZeroU32::MAX)),
            Duration::from_nanos(u64::from(u32::MAX))
        );
    }
}
