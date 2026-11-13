//! What a context source answered a forwarded request with when it failed it.

use crate::answer_verdict::AnswerVerdict;
use andromeda_ngsi_ld_problem_details::batch_operation::BatchProblemDetail;
use http::StatusCode;
use std::fmt;
use thiserror::Error;

/// A context source's non-success answer to one forwarded request.
///
/// Clause 6.3.17 asks for the most informative error, which is the source's own
/// problem document: the answer keeps that document, read as the RFC 7807
/// problem detail it is, and renders its `detail` (or else its `title`) beside
/// the status.
#[derive(Debug, Clone, PartialEq, Error)]
#[error("context source answered HTTP {status}{}", ProblemDetailSuffix(.problem.as_ref()))]
pub struct RemoteAnswer {
    /// The HTTP status the source answered with.
    pub status: u16,
    /// The problem detail the source answered with, when it sent one.
    pub problem: Option<BatchProblemDetail>,
}

impl RemoteAnswer {
    /// Whether the answer is the source's verdict on the request as sent,
    /// which it is unless its status depends on the moment the request came.
    #[must_use]
    pub fn verdict(&self) -> AnswerVerdict {
        match StatusCode::from_u16(self.status) {
            Ok(
                StatusCode::REQUEST_TIMEOUT
                | StatusCode::TOO_MANY_REQUESTS
                | StatusCode::INTERNAL_SERVER_ERROR
                | StatusCode::BAD_GATEWAY
                | StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::GATEWAY_TIMEOUT,
            ) => AnswerVerdict::Withheld,
            Ok(_) | Err(_) => AnswerVerdict::Refused,
        }
    }
}

/// The `: detail` an answer's problem document adds to the status, nothing when
/// it sent none or its detail is empty.
struct ProblemDetailSuffix<'a>(Option<&'a BatchProblemDetail>);

impl fmt::Display for ProblemDetailSuffix<'_> {
    // Manual impl: the suffix is conditional on the presence of a non-empty detail, which no derive expresses.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.map(|problem| problem.detail.as_str()).filter(|detail| !detail.is_empty()) {
            Some(detail) => write!(formatter, ": {detail}"),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{answer_verdict::AnswerVerdict, remote_answer::RemoteAnswer};
    use andromeda_ngsi_ld_problem_details::batch_operation::BatchProblemDetail;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn verdict_of(status: u16) -> AnswerVerdict {
        RemoteAnswer { status, problem: None }.verdict()
    }

    /// RFC 9110 section 15.5.9, RFC 6585 section 4 and RFC 9110 sections
    /// 15.6.1 and 15.6.3 to 15.6.5.
    #[test]
    fn a_timeout_a_rate_limit_and_a_server_or_gateway_failure_withhold_a_verdict() {
        for status in [408, 429, 500, 502, 503, 504] {
            assert_eq!(verdict_of(status), AnswerVerdict::Withheld, "HTTP {status}");
        }
    }

    /// A redirection is not followed, a client error refuses the request as
    /// sent, and a source that does not implement the operation or detects a
    /// loop answers the same request the same way every time.
    #[test]
    fn a_redirection_a_client_error_and_a_lasting_server_refusal_are_verdicts() {
        for status in [307, 400, 401, 403, 404, 409, 413, 415, 422, 501, 508] {
            assert_eq!(verdict_of(status), AnswerVerdict::Refused, "HTTP {status}");
        }
    }

    fn problem(body: &serde_json::Value) -> BatchProblemDetail {
        serde_json::from_value(body.clone()).expect("a problem detail")
    }

    fn rendered(status: u16, problem: Option<BatchProblemDetail>) -> String {
        RemoteAnswer { status, problem }.to_string()
    }

    #[test]
    fn the_rendering_prefers_the_problem_detail_then_the_title_then_the_status() {
        assert_eq!(
            rendered(403, Some(problem(&json!({"type": "https://example.org/denied", "detail": "denied"})))),
            "context source answered HTTP 403: denied"
        );
        assert_eq!(
            rendered(403, Some(problem(&json!({"type": "https://example.org/denied", "title": "Forbidden"})))),
            "context source answered HTTP 403: Forbidden"
        );
        assert_eq!(rendered(403, None), "context source answered HTTP 403");
    }

    #[test]
    fn an_answer_renders_the_way_its_parts_describe_it() {
        let answer = RemoteAnswer {
            status: 409,
            problem: Some(problem(&json!({"type": "https://example.org/exists", "detail": "exists"}))),
        };
        assert_eq!(answer.to_string(), "context source answered HTTP 409: exists");
    }
}
