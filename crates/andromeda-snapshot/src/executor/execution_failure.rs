//! The failures only a Snapshot capture meets, and the problem each is
//! recorded as (clause 5.2.42).
//!
//! A query a Snapshot runs fails, for the most part, as the same query fails
//! through the API: those failures are classified as the API classifies them.
//! What is left is the capture's own: reading the query's whole result, and
//! writing it into the Snapshot. Clause 5.16.1.4 executes the query following
//! clause 5.7.2.4 or 5.7.4.4, so a failure of a context source keeps the
//! clause 6.3.17 type, and one of the capture itself is an `InternalError`.
//!
//! Two failures of context sources have no type of their own: a query whose
//! every matching source failed with no local entity to answer, and a source
//! answer the capture cannot read to the end. Clause 6.3.17 binds 502 only to
//! the single source of an exclusive or redirect registration and gives a
//! failed inclusive source a warning, which an execution result has no place
//! for. Both are recorded as the `BadGateway` 502 Andromeda answers that
//! single source's failure with.

use crate::{error::SnapshotError, executor::instance_walk::WalkRefusal};
use andromeda_context_commit::error::CommitError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_problem_details::{
    error_type::NgsiLdErrorType,
    problem_detail::{ProblemClassification, ProblemDetail},
};
use andromeda_ngsi_ld_representation::attribute_nesting::AttributeNestingExceeded;
use andromeda_temporal::error::TemporalError;
use thiserror::Error;

/// A failure that becomes the problem detail of an execution result
/// (clause 5.2.42), borrowing the typed error it reports.
///
/// The causes are borrowed for the moment the detail is rendered, so they are
/// not `source`s: an error source must be `'static`.
#[derive(Debug, Error)]
pub(super) enum ExecutionFailure<'error> {
    /// The preparation ended in a failure that recurs, so every query reports it.
    #[error("Snapshot preparation failed: {cause}")]
    Preparation {
        /// The failure that ended the preparation.
        cause: &'error SnapshotError,
    },
    /// No local entity matched, and every matching Context Source failed.
    #[error("no valid entity result was available because every matching distributed source failed")]
    EverySourceFailed,
    /// A matching local entity is stored in a form this broker no longer reads,
    /// so a capture without it would not be the result of the query.
    #[error("the stored entity {id} matched the query but does not read back")]
    UnreadableEntity {
        /// The entity that does not read back.
        id: &'error Identifier,
    },
    /// A Context Source answered an entity nested deeper than the Snapshot
    /// may store.
    #[error("an entity a Context Source answered cannot be captured: {cause}")]
    UncapturableSourceEntity {
        /// The nesting limit the entity passed.
        cause: &'error AttributeNestingExceeded,
    },
    /// The captured states could not be turned into branch writes.
    #[error("failed to read results: {cause}")]
    ReadResults {
        /// The failure encoding a captured state.
        cause: &'error CommitError,
    },
    /// The branch could not store the captured entities.
    #[error("failed to store results: {cause}")]
    StoreResults {
        /// The failure committing to the branch.
        cause: &'error CommitError,
    },
    /// The branch holds a later instant for captured entities than every
    /// attempt to store them was stamped at (clause 4.8).
    #[error("{count} captured entities are held by the Snapshot at a later instant than every attempt to store them")]
    PredatedResults {
        /// How many captured entities were not stored.
        count: usize,
    },
    /// The temporal query asked for the aggregated representation, which
    /// records no instance to capture.
    #[error("a Snapshot cannot capture an aggregated temporal representation")]
    AggregatedRepresentation,
    /// The rest of a partial answer of the broker's own history cannot be read.
    #[error(transparent)]
    LocalWalkRefused(WalkRefusal),
    /// The rest of a partial answer a Context Source gave cannot be read.
    #[error(transparent)]
    SourceWalkRefused(WalkRefusal),
    /// The temporal query's pages ran past the offsets a Context Source can be
    /// asked for.
    #[error("the temporal query's pages ran past the offsets a Context Source can be asked for")]
    OffsetOverflow,
    /// The branch could not store the captured temporal evolutions.
    #[error("failed to store temporal results: {cause}")]
    StoreTemporalResults {
        /// The failure recording the history.
        cause: &'error TemporalError,
    },
}

impl ExecutionFailure<'_> {
    /// The clause 5.5.2 category, or the 502 binding, the failure is recorded
    /// as.
    ///
    /// Every matching source failing, and a source whose partial answer cannot
    /// be read on, are recorded as the `BadGateway` 502 Andromeda answers the
    /// failure of a single registered source with (clause 6.3.17). A `lastN` whose
    /// remainder pages backwards cannot be captured whole, which clause 5.5.2
    /// names `TooManyResults`, and an aggregated representation is an
    /// operation a Snapshot does not support. The rest are failures of the
    /// capture itself.
    pub(super) const fn classification(&self) -> ProblemClassification {
        match self {
            ExecutionFailure::EverySourceFailed | ExecutionFailure::SourceWalkRefused(WalkRefusal::Stalled { .. } | WalkRefusal::Unpositioned) => {
                ProblemClassification::BadGateway
            }
            ExecutionFailure::LocalWalkRefused(WalkRefusal::Backward) | ExecutionFailure::SourceWalkRefused(WalkRefusal::Backward) => {
                ProblemClassification::Category(NgsiLdErrorType::TooManyResults)
            }
            ExecutionFailure::AggregatedRepresentation => ProblemClassification::Category(NgsiLdErrorType::OperationNotSupported),
            ExecutionFailure::Preparation { .. }
            | ExecutionFailure::UnreadableEntity { .. }
            | ExecutionFailure::UncapturableSourceEntity { .. }
            | ExecutionFailure::ReadResults { .. }
            | ExecutionFailure::StoreResults { .. }
            | ExecutionFailure::PredatedResults { .. }
            | ExecutionFailure::LocalWalkRefused(WalkRefusal::Stalled { .. } | WalkRefusal::Unpositioned)
            | ExecutionFailure::OffsetOverflow
            | ExecutionFailure::StoreTemporalResults { .. } => ProblemClassification::Category(NgsiLdErrorType::InternalError),
        }
    }

    /// The problem the query's execution result reports.
    pub(super) fn problem(&self) -> ProblemDetail {
        ProblemDetail::from_classification(self.classification(), self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::SnapshotError,
        executor::{execution_failure::ExecutionFailure, instance_walk::WalkRefusal},
        snapshot_invariant::SnapshotInvariant,
    };
    use andromeda_context_commit::error::CommitError;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_ngsi_ld_representation::attribute_nesting::{AttributeNestingExceeded, AttributeNestingLimit};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_temporal::error::TemporalError;
    use chrono::DateTime;
    use pretty_assertions::assert_eq;
    use std::num::NonZeroUsize;

    /// The type and status the problem of `failure` carries.
    fn recorded(failure: &ExecutionFailure<'_>) -> (String, u16) {
        let problem = failure.problem();
        (problem.classification().type_iri().as_str().to_owned(), problem.status_code().as_u16())
    }

    fn stalled() -> WalkRefusal {
        WalkRefusal::Stalled { at: DateTime::UNIX_EPOCH }
    }

    /// A query no local entity answers and whose every matching Context
    /// Source failed is recorded as the `BadGateway` 502 Andromeda answers a
    /// failed single source with (clause 6.3.17).
    #[test]
    fn every_matching_source_failing_is_a_bad_gateway() {
        assert_eq!(
            recorded(&ExecutionFailure::EverySourceFailed),
            ("https://uri.etsi.org/ngsi-ld/errors/BadGateway".to_owned(), 502)
        );
    }

    /// A Context Source whose partial temporal answer cannot be read on is
    /// recorded as the `BadGateway` 502 Andromeda answers a failed single
    /// source with (clause 6.3.17); the broker's own answer failing so is the
    /// capture's `InternalError`.
    #[test]
    fn a_partial_answer_that_cannot_be_read_on_is_the_fault_of_whoever_gave_it() {
        for refusal in [stalled(), WalkRefusal::Unpositioned] {
            assert_eq!(
                recorded(&ExecutionFailure::SourceWalkRefused(refusal)),
                ("https://uri.etsi.org/ngsi-ld/errors/BadGateway".to_owned(), 502),
                "{refusal}"
            );
            assert_eq!(
                recorded(&ExecutionFailure::LocalWalkRefused(refusal)),
                (NgsiLdErrorType::InternalError.type_iri().as_str().to_owned(), 500),
                "{refusal}"
            );
        }
    }

    /// Clause 5.5.2: a `lastN` whose remainder pages backwards, from the
    /// broker or from a Context Source, cannot be captured whole, a 403
    /// `TooManyResults`.
    #[test]
    fn a_last_n_paging_backwards_is_too_many_results() {
        for failure in [
            ExecutionFailure::LocalWalkRefused(WalkRefusal::Backward),
            ExecutionFailure::SourceWalkRefused(WalkRefusal::Backward),
        ] {
            assert_eq!(
                recorded(&failure),
                (NgsiLdErrorType::TooManyResults.type_iri().as_str().to_owned(), 403),
                "{failure}"
            );
        }
    }

    /// Clause 5.5.2: an aggregated temporal representation records no instance
    /// a Snapshot could capture, a 422 `OperationNotSupported`.
    #[test]
    fn an_aggregated_representation_is_operation_not_supported() {
        assert_eq!(
            recorded(&ExecutionFailure::AggregatedRepresentation),
            (NgsiLdErrorType::OperationNotSupported.type_iri().as_str().to_owned(), 422)
        );
    }

    /// Clause 5.5.2: a failure of the capture itself is an `InternalError`
    /// during the operation execution, a 500.
    #[test]
    fn a_failure_of_the_capture_itself_is_an_internal_error() {
        let identifier = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("a valid identifier");
        let nesting = AttributeNestingExceeded {
            attribute: "https://example.org/reading".parse().expect("an attribute name"),
            limit: AttributeNestingLimit::new(NonZeroUsize::MIN),
        };
        let commit = CommitError::Persistence(PersistenceFailure::StatementRejected);
        let temporal = TemporalError::Persistence(PersistenceFailure::StatementRejected);
        let preparation = SnapshotError::from(SnapshotInvariant::LeaseDeadlineOverflow);
        for failure in [
            ExecutionFailure::Preparation { cause: &preparation },
            ExecutionFailure::UnreadableEntity { id: &identifier },
            ExecutionFailure::UncapturableSourceEntity { cause: &nesting },
            ExecutionFailure::ReadResults { cause: &commit },
            ExecutionFailure::StoreResults { cause: &commit },
            ExecutionFailure::PredatedResults { count: 2 },
            ExecutionFailure::OffsetOverflow,
            ExecutionFailure::StoreTemporalResults { cause: &temporal },
        ] {
            assert_eq!(
                recorded(&failure),
                (NgsiLdErrorType::InternalError.type_iri().as_str().to_owned(), 500),
                "{failure}"
            );
        }
    }

    /// The problem's detail is the failure's own text.
    #[test]
    fn the_problem_detail_is_the_failure_text() {
        let failure = ExecutionFailure::EverySourceFailed;
        assert_eq!(failure.problem().detail(), failure.to_string());
    }
}
