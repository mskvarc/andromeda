//! What a context source answered a forwarded request with when it failed it.

use serde_json::Value;
use thiserror::Error;

/// A context source's non-success answer to one forwarded request.
///
/// Clause 6.3.17 asks for the most informative error, which is the source's own
/// problem document: the answer keeps that document as the source sent it, and
/// renders its `detail` (or else its `title`) beside the status.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{}", describe_remote_answer(*.status, .body.as_ref()))]
pub struct RemoteAnswer {
    /// The HTTP status the source answered with.
    pub status: u16,
    /// The problem document the source answered with, when it sent one.
    pub body: Option<Value>,
}

/// The most informative rendering of a remote answer: its problem `detail`, its
/// `title`, or the bare status.
#[must_use]
pub fn describe_remote_answer(status: u16, body: Option<&Value>) -> String {
    body.and_then(|body| body.get("detail").or_else(|| body.get("title")))
        .and_then(Value::as_str)
        .map_or_else(
            || format!("context source answered HTTP {status}"),
            |detail| format!("context source answered HTTP {status}: {detail}"),
        )
}

#[cfg(test)]
mod tests {
    use crate::remote_answer::{RemoteAnswer, describe_remote_answer};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn the_rendering_prefers_the_problem_detail_then_the_title_then_the_status() {
        assert_eq!(
            describe_remote_answer(403, Some(&json!({"detail": "denied"}))),
            "context source answered HTTP 403: denied"
        );
        assert_eq!(
            describe_remote_answer(403, Some(&json!({"title": "Forbidden"}))),
            "context source answered HTTP 403: Forbidden"
        );
        assert_eq!(describe_remote_answer(403, None), "context source answered HTTP 403");
    }

    #[test]
    fn an_answer_renders_the_way_its_parts_describe_it() {
        let answer = RemoteAnswer {
            status: 409,
            body: Some(json!({"detail": "exists"})),
        };
        assert_eq!(answer.to_string(), "context source answered HTTP 409: exists");
    }
}
