//! Recording a failed temporal step of a snapshot query (clause 5.2.42).
//!
//! A query's own failure is recorded in its execution result, a failed read
//! as the API reports the same failure. A transient store failure is not the
//! query's: it is returned as an error instead, so the whole preparation
//! restarts.

use crate::{
    error::{Recurrence, SnapshotError, temporal_recurrence},
    executor::{execution_failure::ExecutionFailure, status::failure_detail},
};
use andromeda_failure_classification::temporal_failure::classify_temporal_error;
use andromeda_ngsi_ld_model::snapshot::ExecutionResultDetails;
use andromeda_temporal::error::TemporalError;

/// Which temporal operation of a snapshot query failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TemporalStep {
    /// Reading a page of temporal entities.
    Query,
    /// Writing a captured evolution into the branch.
    Store,
}

/// A failed temporal query or temporal write, `step` naming which: the
/// query's own failure, or a transient store failure the preparation restarts
/// on.
pub(crate) fn temporal_failure(error: TemporalError, step: TemporalStep) -> Result<ExecutionResultDetails, SnapshotError> {
    match temporal_recurrence(&error) {
        Recurrence::Transient => Err(SnapshotError::from(error)),
        Recurrence::Persistent => Ok(failure_detail(match step {
            TemporalStep::Query => classify_temporal_error(&error).into_problem(),
            TemporalStep::Store => ExecutionFailure::StoreTemporalResults { cause: &error }.problem(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use crate::executor::temporal_failure::{TemporalStep, temporal_failure};
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_temporal::{error::TemporalError, result_bound::ResultBound};
    use pretty_assertions::assert_eq;

    /// The classification a failed step records.
    fn classification(error: TemporalError, step: TemporalStep) -> Option<ProblemClassification> {
        temporal_failure(error, step)
            .expect("the query's own failure")
            .problem_details
            .map(|problem| problem.classification())
    }

    /// Clause 5.16.1.4 executes a temporal query following clause 5.7.4.4: a
    /// read that fails records the type Query Temporal Evolution of Entities
    /// answers it with.
    #[test]
    fn a_failed_temporal_read_records_the_problem_the_api_answers_with() {
        assert_eq!(
            classification(TemporalError::too_many_results(500, 100, ResultBound::AggregationPeriods), TemporalStep::Query),
            Some(ProblemClassification::Category(NgsiLdErrorType::TooManyResults))
        );
    }

    /// Writing the captured history into the Snapshot is the capture's own
    /// step, so whatever failed it records an `InternalError`.
    #[test]
    fn a_failed_temporal_write_is_an_internal_error() {
        assert_eq!(
            classification(TemporalError::too_many_results(500, 100, ResultBound::AggregationPeriods), TemporalStep::Store),
            Some(ProblemClassification::Category(NgsiLdErrorType::InternalError))
        );
        assert_eq!(
            classification(TemporalError::Persistence(PersistenceFailure::StatementRejected), TemporalStep::Store),
            Some(ProblemClassification::Category(NgsiLdErrorType::InternalError))
        );
    }

    /// A transient store failure is not the query's: the preparation restarts.
    #[test]
    fn a_transient_temporal_failure_restarts_the_preparation() {
        for step in [TemporalStep::Query, TemporalStep::Store] {
            assert!(temporal_failure(TemporalError::StoreSaturated, step).is_err());
        }
    }
}
