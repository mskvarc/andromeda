//! How long a failed notification delivery waits before it becomes due again.

/// Exponential retry backoff for a delivery that has already failed
/// `attempt_count` times.
///
/// The delay is `base_backoff_secs * 2^min(attempt_count, max_attempts)`, so
/// the growth stops once a delivery has reached its retry limit and cannot be
/// claimed again anyway. Saturating arithmetic keeps an absurd configured base
/// from overflowing rather than panicking.
#[must_use]
pub fn backoff_seconds(base_backoff_secs: i64, attempt_count: i32, max_attempts: i32) -> i64 {
    let bounded_attempt = attempt_count.min(max_attempts).max(0);
    let exponent = bounded_attempt.unsigned_abs();
    base_backoff_secs.saturating_mul(2_i64.saturating_pow(exponent))
}

#[cfg(test)]
mod tests {
    use crate::delivery_backoff::backoff_seconds;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_first_attempt_waits_exactly_the_base_backoff() {
        assert_eq!(backoff_seconds(5, 0, 5), 5);
    }

    #[test]
    fn each_further_attempt_doubles_the_wait() {
        assert_eq!(backoff_seconds(5, 1, 5), 10);
        assert_eq!(backoff_seconds(5, 2, 5), 20);
        assert_eq!(backoff_seconds(5, 3, 5), 40);
    }

    #[test]
    fn the_wait_stops_growing_at_the_retry_limit() {
        assert_eq!(backoff_seconds(5, 5, 5), 160);
        assert_eq!(backoff_seconds(5, 10, 5), 160);
    }

    #[test]
    fn an_enormous_base_backoff_saturates_instead_of_overflowing() {
        assert!(backoff_seconds(i64::MAX / 2, 10, 10) > 0);
    }

    #[test]
    fn a_zero_base_backoff_makes_a_retry_due_immediately() {
        assert_eq!(backoff_seconds(0, 5, 5), 0);
    }
}
