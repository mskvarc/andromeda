//! How a delay is spread around its bound, so that callers which failed
//! together do not retry together.
//!
//! This is the only place in the workspace that draws a random number for a
//! retry.

use std::time::Duration;

/// Nanoseconds in one second, the split a drawn nanosecond count is rebuilt
/// from.
const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// Where a delay is drawn relative to its un-jittered bound.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Jitter {
    /// Exactly the bound.
    #[default]
    None,
    /// Uniformly from zero to the bound. Colliding callers are spread over
    /// the whole interval, which separates them fastest.
    Full,
    /// The bound plus a uniform draw up to the bound, never past the ceiling.
    /// The delay never falls below the bound, for callers whose bound is a
    /// minimum quiet period.
    Additive,
}

impl Jitter {
    /// The delay drawn for `bound`, never longer than `ceiling` unless `bound`
    /// itself is.
    #[must_use]
    pub fn apply(self, bound: Duration, ceiling: Duration) -> Duration {
        match self {
            Jitter::None => bound,
            Jitter::Full => uniform_up_to(bound),
            Jitter::Additive => bound.saturating_add(uniform_up_to(bound)).min(ceiling.max(bound)),
        }
    }
}

/// A duration drawn uniformly from zero to `bound`, both included.
fn uniform_up_to(bound: Duration) -> Duration {
    let nanos = fastrand::u128(0..=bound.as_nanos());
    // The draw never exceeds the nanoseconds of a valid `Duration`, so both
    // conversions succeed; the bound stands in only to keep the call total.
    match (u64::try_from(nanos / NANOS_PER_SECOND), u32::try_from(nanos % NANOS_PER_SECOND)) {
        (Ok(seconds), Ok(subsecond)) => Duration::new(seconds, subsecond),
        (Err(_), Ok(_) | Err(_)) | (Ok(_), Err(_)) => bound,
    }
}

#[cfg(test)]
mod tests {
    use crate::jitter::Jitter;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    const BOUND: Duration = Duration::from_millis(40);
    const CEILING: Duration = Duration::from_millis(60);

    #[test]
    fn no_jitter_returns_the_bound() {
        assert_eq!(Jitter::None.apply(BOUND, CEILING), BOUND);
    }

    #[test]
    fn full_jitter_stays_between_zero_and_the_bound() {
        for _ in 0..1000 {
            assert!(Jitter::Full.apply(BOUND, CEILING) <= BOUND);
        }
    }

    #[test]
    fn full_jitter_of_a_zero_bound_is_zero() {
        assert_eq!(Jitter::Full.apply(Duration::ZERO, CEILING), Duration::ZERO);
    }

    #[test]
    fn additive_jitter_stays_between_the_bound_and_the_ceiling() {
        for _ in 0..1000 {
            let delay = Jitter::Additive.apply(BOUND, CEILING);
            assert!(delay >= BOUND, "{delay:?} fell below the bound");
            assert!(delay <= CEILING, "{delay:?} passed the ceiling");
        }
    }

    #[test]
    fn additive_jitter_of_a_bound_at_the_ceiling_is_the_ceiling() {
        assert_eq!(Jitter::Additive.apply(CEILING, CEILING), CEILING);
    }

    #[test]
    fn a_draw_from_the_longest_duration_does_not_panic() {
        assert!(Jitter::Full.apply(Duration::MAX, Duration::MAX) <= Duration::MAX);
    }
}
