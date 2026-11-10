//! Snapshot execution result aggregation and status selection.

use crate::executor::source_shortfall::SourceShortfall;
use andromeda_ngsi_ld_model::snapshot::{ExecutionResultDetails, ExecutionResultStatus, SnapshotStatus};
use andromeda_ngsi_ld_problem_details::problem_detail::ProblemDetail;

/// The execution result details of every query of one Snapshot, from which
/// its final status is decided (ETSI GS CIM 009 v1.9.1 clause 5.16.1.4).
#[derive(Default)]
pub(crate) struct ExecutionAggregate {
    /// One detail per entity query, in query order.
    query_details: Vec<ExecutionResultDetails>,
    /// One detail per temporal query, in query order.
    temporal_query_details: Vec<ExecutionResultDetails>,
    /// Some query succeeded, with or without results.
    any_success: bool,
    /// Some query succeeded and captured at least one entity.
    any_success_with_results: bool,
    /// Some query failed.
    any_failure: bool,
}

impl ExecutionAggregate {
    /// Record the outcome of the next entity query.
    pub(crate) fn push_entity(&mut self, detail: ExecutionResultDetails) {
        self.update_flags(&detail);
        self.query_details.push(detail);
    }

    /// Record the outcome of the next temporal query.
    pub(crate) fn push_temporal(&mut self, detail: ExecutionResultDetails) {
        self.update_flags(&detail);
        self.temporal_query_details.push(detail);
    }

    /// `success` when one query captured data and none failed, `partial` when
    /// one captured data and another failed, `empty` when queries succeeded
    /// without data, `failure` when none succeeded or none ran.
    pub(crate) const fn final_status(&self) -> SnapshotStatus {
        if self.any_success_with_results && !self.any_failure {
            SnapshotStatus::Success
        } else if self.any_success_with_results && self.any_failure {
            SnapshotStatus::Partial
        } else if self.any_success {
            SnapshotStatus::Empty
        } else {
            SnapshotStatus::Failure
        }
    }

    /// The entity and temporal query details, each `None` when no query of
    /// that kind ran.
    pub(crate) fn detail_slices(&self) -> (Option<&[ExecutionResultDetails]>, Option<&[ExecutionResultDetails]>) {
        let query_details = (!self.query_details.is_empty()).then_some(self.query_details.as_slice());
        let temporal_query_details = (!self.temporal_query_details.is_empty()).then_some(self.temporal_query_details.as_slice());
        (query_details, temporal_query_details)
    }

    /// Fold one detail's status into the success and failure flags.
    const fn update_flags(&mut self, detail: &ExecutionResultDetails) {
        match detail.result_status {
            ExecutionResultStatus::Success => {
                self.any_success = true;
                self.any_success_with_results = true;
            }
            ExecutionResultStatus::Failure => self.any_failure = true,
            ExecutionResultStatus::Empty => self.any_success = true,
        }
    }
}

/// The detail of a query that ran: `success` when it captured `count > 0`
/// entities, `empty` otherwise.
pub(crate) const fn success_detail(count: usize) -> ExecutionResultDetails {
    ExecutionResultDetails {
        result_status: if count > 0 {
            ExecutionResultStatus::Success
        } else {
            ExecutionResultStatus::Empty
        },
        problem_details: None,
    }
}

/// The detail of a query whose whole answer was captured: `success` when it
/// captured `count > 0` entities, `empty` otherwise, with the problem naming
/// the Context Sources `shortfall` holds failed for it (clause 5.2.42).
pub(super) fn answered_detail(count: usize, shortfall: &SourceShortfall) -> ExecutionResultDetails {
    ExecutionResultDetails {
        problem_details: shortfall.problem(),
        ..success_detail(count)
    }
}

/// The detail of a failed query, reporting `problem` with the type the
/// failure was classified as (clause 5.2.42).
pub(crate) const fn failure_detail(problem: ProblemDetail) -> ExecutionResultDetails {
    ExecutionResultDetails {
        result_status: ExecutionResultStatus::Failure,
        problem_details: Some(problem),
    }
}

#[cfg(test)]
mod tests {
    use crate::executor::{
        source_shortfall::SourceShortfall,
        status::{ExecutionAggregate, answered_detail, failure_detail, success_detail},
    };
    use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
    use andromeda_ngsi_ld_model::snapshot::{ExecutionResultDetails, ExecutionResultStatus, SnapshotStatus};
    use andromeda_ngsi_ld_problem_details::{
        error_type::NgsiLdErrorType,
        problem_detail::{ProblemClassification, ProblemDetail},
    };

    fn failed(detail: &str) -> ExecutionResultDetails {
        failure_detail(ProblemDetail::new(NgsiLdErrorType::InternalError, detail))
    }

    #[test]
    fn no_results_produce_empty_status() {
        let mut aggregate = ExecutionAggregate::default();
        aggregate.push_entity(success_detail(0));
        pretty_assertions::assert_eq!(aggregate.final_status(), SnapshotStatus::Empty);
    }

    #[test]
    fn a_non_empty_result_produces_success_status() {
        let mut aggregate = ExecutionAggregate::default();
        aggregate.push_entity(success_detail(1));
        pretty_assertions::assert_eq!(aggregate.final_status(), SnapshotStatus::Success);
    }

    #[test]
    fn an_empty_result_with_a_failure_stays_empty() {
        let mut aggregate = ExecutionAggregate::default();
        aggregate.push_entity(success_detail(0));
        aggregate.push_temporal(failed("unavailable source"));
        pretty_assertions::assert_eq!(aggregate.final_status(), SnapshotStatus::Empty);
    }

    #[test]
    fn usable_data_with_a_failure_is_partial() {
        let mut aggregate = ExecutionAggregate::default();
        aggregate.push_entity(success_detail(2));
        aggregate.push_temporal(failed("unavailable source"));
        pretty_assertions::assert_eq!(aggregate.final_status(), SnapshotStatus::Partial);
    }

    #[test]
    fn failures_without_a_success_are_failure() {
        let mut aggregate = ExecutionAggregate::default();
        aggregate.push_entity(failed("invalid query"));
        pretty_assertions::assert_eq!(aggregate.final_status(), SnapshotStatus::Failure);
    }

    /// Clause 5.2.42: a failed query reports the problem it was classified
    /// as, whatever its type.
    #[test]
    fn a_failure_reports_the_problem_it_was_classified_as() {
        let detail = failure_detail(ProblemDetail::from_classification(
            ProblemClassification::BadGateway,
            "a source answer cannot be read on",
        ));
        pretty_assertions::assert_eq!(detail.result_status, ExecutionResultStatus::Failure);
        pretty_assertions::assert_eq!(
            detail.problem_details.map(|problem| problem.classification()),
            Some(ProblemClassification::BadGateway)
        );
    }

    #[test]
    fn no_executions_are_failure() {
        pretty_assertions::assert_eq!(ExecutionAggregate::default().final_status(), SnapshotStatus::Failure);
    }

    #[test]
    fn a_query_every_source_answered_records_its_status_without_a_problem() {
        let shortfall = SourceShortfall::default();
        for (count, status) in [(0, ExecutionResultStatus::Empty), (2, ExecutionResultStatus::Success)] {
            let detail = answered_detail(count, &shortfall);
            pretty_assertions::assert_eq!(detail.result_status, status);
            assert!(detail.problem_details.is_none());
        }
    }

    /// Clause 5.2.42: the answer reports no error when a source failed, so the
    /// status follows what was captured and the problem says which sources
    /// are missing.
    #[test]
    fn a_query_a_source_failed_for_records_its_status_with_the_problem() {
        let mut shortfall = SourceShortfall::default();
        shortfall.record(&[NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "a source did not respond")]);
        for (count, status) in [(0, ExecutionResultStatus::Empty), (2, ExecutionResultStatus::Success)] {
            let detail = answered_detail(count, &shortfall);
            pretty_assertions::assert_eq!(detail.result_status, status);
            pretty_assertions::assert_eq!(detail.problem_details, shortfall.problem());
        }
    }

    /// A problem beside an empty result does not make the query a failure, so
    /// the Snapshot is empty.
    #[test]
    fn an_empty_result_a_source_failed_for_is_empty() {
        let mut shortfall = SourceShortfall::default();
        shortfall.record(&[NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "a source did not respond")]);
        let mut aggregate = ExecutionAggregate::default();
        aggregate.push_entity(answered_detail(0, &shortfall));
        pretty_assertions::assert_eq!(aggregate.final_status(), SnapshotStatus::Empty);
    }
}
