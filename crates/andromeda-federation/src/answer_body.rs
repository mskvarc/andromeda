//! The body is kept as the text the source wrote, and each reader decodes the typed resource it expects from it. A 207 `UpdateResult` of a single-entity write is read once, its names expanded into the Fully Qualified Names clause 6.3.6 asks for.

use andromeda_ngsi_ld_model::update_result::UpdateResult;
use andromeda_ngsi_ld_problem_details::batch_operation::BatchProblemDetail;
use serde::de::DeserializeOwned;
use std::sync::Arc;

/// What a context source answered a forwarded write with.
#[derive(Debug, Clone, PartialEq)]
pub enum AnswerBody {
    /// No body, or one that is not JSON.
    Absent,
    /// A JSON body, as the source wrote it.
    Text(Arc<[u8]>),
    /// The clause 5.2.18 `UpdateResult` of a 207, its names Fully Qualified.
    UpdateResult(UpdateResult),
    /// The problem detail a batch answer reported for one entity (clause
    /// 5.2.17).
    Problem(BatchProblemDetail),
}

impl AnswerBody {
    /// The body decoded as a `T`, when it is JSON text that decodes as one.
    #[must_use]
    pub fn decoded<T: DeserializeOwned>(&self) -> Option<T> {
        match self {
            AnswerBody::Text(text) => serde_json::from_slice(text).ok(),
            AnswerBody::Absent | AnswerBody::UpdateResult(_) | AnswerBody::Problem(_) => None,
        }
    }

    /// The problem detail the body is: the one a batch reported, or the body's
    /// text read as one (RFC 7807).
    #[must_use]
    pub fn problem(&self) -> Option<BatchProblemDetail> {
        match self {
            AnswerBody::Problem(problem) => Some(problem.clone()), // clone: the caller owns the problem it reports
            AnswerBody::Text(text) => BatchProblemDetail::from_answer(text),
            AnswerBody::Absent | AnswerBody::UpdateResult(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::answer_body::AnswerBody;
    use andromeda_ngsi_ld_problem_details::batch_operation::BatchOperationResult;
    use std::sync::Arc;

    #[test]
    fn a_text_body_decodes_as_the_resource_it_is() {
        let body = AnswerBody::Text(Arc::from(br#"{"success":["urn:ngsi-ld:Vehicle:1"],"errors":[]}"#.as_slice()));
        let result: BatchOperationResult = body.decoded().expect("a batch result");
        pretty_assertions::assert_eq!(result.success.len(), 1);
        assert!(AnswerBody::Absent.decoded::<BatchOperationResult>().is_none());
    }

    /// Clause 6.3.17: a source's explanation is the most informative cause even
    /// when its document names no `type`.
    #[test]
    fn a_text_body_naming_only_a_detail_is_the_problem_the_source_reported() {
        let body = AnswerBody::Text(Arc::from(br#"{"detail":"no access"}"#.as_slice()));
        pretty_assertions::assert_eq!(body.problem().map(|problem| problem.detail), Some("no access".to_owned()));
        assert!(AnswerBody::Absent.problem().is_none());
    }
}
