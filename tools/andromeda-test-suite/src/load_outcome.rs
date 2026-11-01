//! How one request of a workload resolved: the status the broker answered with and the delay a `Retry-After` named,
//! or what became of a request that got no answer.
//!
//! A workload's assertions are about statuses and the retry delay a 503 carries (ETSI GS CIM 009 v1.9.1 clause 5.5.6
//! with IETF RFC 9110 section 10.2.3), and about requests left without an answer, which a broker under strain must
//! never produce.

use crate::{answered_status::AnsweredStatus, connection_outcome::ConnectionOutcome};
use http::{HeaderMap, StatusCode, header::RETRY_AFTER};
use ngsi_ld::client::{ClientError, ReqwestError};
use std::time::Duration;

/// What a `Retry-After` field asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryAfter {
    /// Ask again after this delay.
    Delay(Duration),
    /// A value that is not a delay in seconds: an HTTP-date, or text that is neither.
    NotADelay,
}

impl RetryAfter {
    /// The `Retry-After` of `headers`, if they carry one.
    #[must_use]
    pub fn of(headers: &HeaderMap) -> Option<RetryAfter> {
        let value = headers.get(RETRY_AFTER)?;
        Some(
            value
                .to_str()
                .ok()
                .and_then(|text| text.trim().parse::<u64>().ok())
                .map_or(RetryAfter::NotADelay, |seconds| RetryAfter::Delay(Duration::from_secs(seconds))),
        )
    }
}

/// How one request resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadOutcome {
    /// The broker answered.
    Answered {
        /// The status.
        status: StatusCode,
        /// The answer's `Retry-After`, if it carried one.
        retry_after: Option<RetryAfter>,
    },
    /// No answer arrived.
    Unanswered(ConnectionOutcome),
}

impl LoadOutcome {
    /// How the request whose result is `result` resolved.
    #[must_use]
    pub fn of<T>(result: &Result<T, ClientError<ReqwestError>>) -> LoadOutcome
    where
        Result<T, ClientError<ReqwestError>>: AnsweredStatus,
    {
        match result.answered_status() {
            Some(status) => LoadOutcome::Answered {
                status,
                retry_after: result.answered_headers().and_then(RetryAfter::of),
            },
            None => LoadOutcome::Unanswered(ConnectionOutcome::of_request(result)),
        }
    }

    /// The status, when the broker answered.
    #[must_use]
    pub const fn status(&self) -> Option<StatusCode> {
        match self {
            LoadOutcome::Answered { status, .. } => Some(*status),
            LoadOutcome::Unanswered(_) => None,
        }
    }

    /// Whether the broker answered with a success.
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.status().is_some_and(|status| status.is_success())
    }

    /// Whether the broker answered 503 naming a delay to ask again after.
    #[must_use]
    pub const fn is_paced_unavailability(&self) -> bool {
        matches!(
            self,
            LoadOutcome::Answered {
                status: StatusCode::SERVICE_UNAVAILABLE,
                retry_after: Some(RetryAfter::Delay(_)),
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::load_outcome::{LoadOutcome, RetryAfter};
    use googletest::prelude::*;
    use http::{HeaderMap, HeaderValue, StatusCode, header::RETRY_AFTER};
    use std::time::Duration;

    #[gtest]
    fn a_delay_in_seconds_reads_as_a_delay_and_a_date_as_none() {
        let mut headers = HeaderMap::new();
        expect_that!(RetryAfter::of(&headers), none());
        headers.insert(RETRY_AFTER, HeaderValue::from_static("5"));
        expect_that!(RetryAfter::of(&headers), some(eq(RetryAfter::Delay(Duration::from_secs(5)))));
        headers.insert(RETRY_AFTER, HeaderValue::from_static("Fri, 31 Dec 1999 23:59:59 GMT"));
        expect_that!(RetryAfter::of(&headers), some(eq(RetryAfter::NotADelay)));
    }

    #[gtest]
    fn only_a_503_with_a_delay_is_paced() {
        let paced = LoadOutcome::Answered {
            status: StatusCode::SERVICE_UNAVAILABLE,
            retry_after: Some(RetryAfter::Delay(Duration::from_secs(1))),
        };
        let unpaced = LoadOutcome::Answered {
            status: StatusCode::SERVICE_UNAVAILABLE,
            retry_after: None,
        };
        expect_that!(paced.is_paced_unavailability(), eq(true));
        expect_that!(unpaced.is_paced_unavailability(), eq(false));
        expect_that!(paced.is_success(), eq(false));
    }
}
