//! How a failed delivery attempt bears on attempting the same notification
//! again.
//!
//! An endpoint that was not reached, or answered that it cannot take a request
//! now, may take the same notification once it recovers, so the delivery is
//! worth retrying for as long as an outage lasts. An endpoint that was reached
//! and answered any other server error failed on this notification: a later
//! attempt may succeed, but one it keeps failing holds back every notification
//! queued after it, so the delivery is retried for less time. Every other
//! failure repeats on every attempt.

/// What a failed attempt says about the next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureClass {
    /// The endpoint was not reached, or answered `429 Too Many Requests` (RFC
    /// 6585 section 4), `502 Bad Gateway`, `503 Service Unavailable` or `504
    /// Gateway Timeout` (RFC 9110 sections 15.6.3 to 15.6.5): it cannot take a
    /// request now, and may once it recovers.
    Unavailable,
    /// The endpoint answered another server error: it was reached and failed
    /// on this notification.
    ServerError,
    /// Attempting the same notification again fails the same way.
    Permanent,
}

impl FailureClass {
    /// The class of an HTTP answer with the non-success `status`.
    #[must_use]
    pub const fn of_http_status(status: u16) -> FailureClass {
        match status {
            429 | 502 | 503 | 504 => FailureClass::Unavailable,
            500..=599 => FailureClass::ServerError,
            _ => FailureClass::Permanent,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::failure_class::FailureClass;
    use pretty_assertions::assert_eq;

    #[test]
    fn an_answer_saying_the_endpoint_cannot_take_a_request_now_is_unavailability() {
        for status in [429, 502, 503, 504] {
            assert_eq!(FailureClass::of_http_status(status), FailureClass::Unavailable, "{status}");
        }
    }

    #[test]
    fn every_other_server_error_is_one_the_endpoint_failed_on() {
        for status in [500, 501, 505, 507, 599] {
            assert_eq!(FailureClass::of_http_status(status), FailureClass::ServerError, "{status}");
        }
    }

    #[test]
    fn a_client_error_or_an_unfollowed_redirect_repeats_on_every_attempt() {
        for status in [301, 400, 401, 404, 413, 422, 428, 431] {
            assert_eq!(FailureClass::of_http_status(status), FailureClass::Permanent, "{status}");
        }
    }
}
