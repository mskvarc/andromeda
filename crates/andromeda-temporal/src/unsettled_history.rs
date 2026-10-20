//! A Temporal API mutation refused while its entity's queued history is held
//! by another delivery owner, and when the client may retry it.

use std::time::Duration;

/// The retry delay of a mutation whose barrier ran out of time while another
/// delivery owner held the entity's queued history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnsettledHistory {
    /// Whole seconds, at least one.
    retry_after: Duration,
}

impl UnsettledHistory {
    /// The refusal of a barrier whose holder's lease lapses in
    /// `remaining_lease`.
    ///
    /// `Retry-After` carries whole seconds (RFC 9110 section 10.2.3), so the
    /// lease is rounded up, and a lease that has already lapsed still asks for
    /// one second: the client is told to come back once the barrier can claim
    /// the history itself.
    #[must_use]
    pub const fn after(remaining_lease: Duration) -> UnsettledHistory {
        let whole_seconds = if remaining_lease.subsec_nanos() > 0 {
            remaining_lease.as_secs().saturating_add(1)
        } else {
            remaining_lease.as_secs()
        };
        let whole_seconds = if whole_seconds == 0 { 1 } else { whole_seconds };
        UnsettledHistory {
            retry_after: Duration::from_secs(whole_seconds),
        }
    }

    /// The delay the client is asked to wait before retrying.
    #[must_use]
    pub const fn retry_after(self) -> Duration {
        self.retry_after
    }
}

#[cfg(test)]
mod tests {
    use crate::unsettled_history::UnsettledHistory;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn the_retry_delay_is_the_remaining_lease_rounded_up_to_whole_seconds_and_never_zero() {
        for (remaining_lease, retry_after) in [
            (Duration::ZERO, 1),
            (Duration::from_millis(200), 1),
            (Duration::from_secs(1), 1),
            (Duration::from_millis(1100), 2),
            (Duration::from_millis(29_010), 30),
        ] {
            assert_eq!(
                UnsettledHistory::after(remaining_lease).retry_after(),
                Duration::from_secs(retry_after),
                "{remaining_lease:?}"
            );
        }
    }
}
