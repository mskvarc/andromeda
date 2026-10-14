//! What a context source answered a forwarded request with when it failed it.

use andromeda_ngsi_ld_problem_details::batch_operation::BatchProblemDetail;
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
    use crate::remote_answer::RemoteAnswer;
    use andromeda_ngsi_ld_problem_details::batch_operation::BatchProblemDetail;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn problem(body: &serde_json::Value) -> BatchProblemDetail {
        serde_json::from_value(body.clone()).expect("a problem detail") // clone: the fixture stays readable
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
