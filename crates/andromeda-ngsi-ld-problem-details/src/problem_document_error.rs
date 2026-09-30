//! Why a `ProblemDetails` document read back from a peer is not a coherent NGSI-LD problem.

use thiserror::Error;

/// A `ProblemDetails` document whose members contradict one another.
///
/// A broker reading back a Context Source's or a peer broker's answer
/// (clause 5.5.2) accepts only a document whose type, status and title agree,
/// so each disagreement is reported as its own typed reason.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProblemDocumentError {
    /// The status is not the one the clause 6.3.17 distributed type fixes.
    #[error("problem status disagrees with its distributed NGSI-LD type")]
    DistributedStatusMismatch,
    /// The type IRI names none of the clause 5.5.2 error types.
    #[error("unknown NGSI-LD problem type")]
    UnknownType,
    /// The title is not the clause 5.5.3 title of the named type.
    #[error("problem title disagrees with its NGSI-LD type")]
    TitleMismatch,
}

#[cfg(test)]
mod tests {
    use crate::problem_document_error::ProblemDocumentError;

    #[test]
    fn each_disagreement_reads_as_its_own_lowercase_reason() {
        pretty_assertions::assert_eq!(
            ProblemDocumentError::DistributedStatusMismatch.to_string(),
            "problem status disagrees with its distributed NGSI-LD type"
        );
        pretty_assertions::assert_eq!(ProblemDocumentError::UnknownType.to_string(), "unknown NGSI-LD problem type");
        pretty_assertions::assert_eq!(ProblemDocumentError::TitleMismatch.to_string(), "problem title disagrees with its NGSI-LD type");
    }
}
