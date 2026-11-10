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
}

impl ExecutionAggregate {
    /// Record the outcome of the next entity query.
    pub(crate) fn push_entity(&mut self, detail: ExecutionResultDetails) {
        self.query_details.push(detail);
    }

    /// Record the outcome of the next temporal query.
    pub(crate) fn push_temporal(&mut self, detail: ExecutionResultDetails) {
        self.temporal_query_details.push(detail);
    }

    /// The rules of clause 5.16.1.4, taken in order over entity and temporal
    /// queries alike: `success` when every query yielded a result, `partial`
    /// when some query yielded one, `empty` when some query succeeded without
    /// one, and `failure` otherwise, which includes a Snapshot no query of
    /// which ran.
    pub(crate) fn final_status(&self) -> SnapshotStatus {
        let some = |status: ExecutionResultStatus| self.statuses().any(|ran| ran == status);
        if some(ExecutionResultStatus::Success) && self.statuses().all(|ran| ran == ExecutionResultStatus::Success) {
            SnapshotStatus::Success
        } else if some(ExecutionResultStatus::Success) {
            SnapshotStatus::Partial
        } else if some(ExecutionResultStatus::Empty) {
            SnapshotStatus::Empty
        } else {
            SnapshotStatus::Failure
        }
    }

    /// The result status of every query, entity queries first.
    fn statuses(&self) -> impl Iterator<Item = ExecutionResultStatus> {
        self.query_details.iter().chain(&self.temporal_query_details).map(|detail| detail.result_status)
    }

    /// The entity and temporal query details, each `None` when no query of
    /// that kind ran.
    pub(crate) fn detail_slices(&self) -> (Option<&[ExecutionResultDetails]>, Option<&[ExecutionResultDetails]>) {
        let query_details = (!self.query_details.is_empty()).then_some(self.query_details.as_slice());
        let temporal_query_details = (!self.temporal_query_details.is_empty()).then_some(self.temporal_query_details.as_slice());
        (query_details, temporal_query_details)
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

    /// The detail of a query that ended in `status`.
    fn ended(status: ExecutionResultStatus) -> ExecutionResultDetails {
        match status {
            ExecutionResultStatus::Success => success_detail(1),
            ExecutionResultStatus::Empty => success_detail(0),
            ExecutionResultStatus::Failure => failed("unavailable source"),
        }
    }

    /// Clause 5.16.1.4 takes its rules in order, so a query without a result
    /// beside one with a result makes the Snapshot partial whether or not a
    /// third failed. Every status is recorded by an entity query and by a
    /// temporal query in turn, as the rules count both alike.
    #[test]
    fn the_status_follows_the_rules_in_order_over_every_combination_of_query_results() {
        let cases: [(&[ExecutionResultStatus], SnapshotStatus); 10] = [
            (&[ExecutionResultStatus::Success], SnapshotStatus::Success),
            (&[ExecutionResultStatus::Success, ExecutionResultStatus::Success], SnapshotStatus::Success),
            (&[ExecutionResultStatus::Empty], SnapshotStatus::Empty),
            (&[ExecutionResultStatus::Empty, ExecutionResultStatus::Empty], SnapshotStatus::Empty),
            (&[ExecutionResultStatus::Failure], SnapshotStatus::Failure),
            (&[ExecutionResultStatus::Failure, ExecutionResultStatus::Failure], SnapshotStatus::Failure),
            (&[ExecutionResultStatus::Success, ExecutionResultStatus::Empty], SnapshotStatus::Partial),
            (&[ExecutionResultStatus::Success, ExecutionResultStatus::Failure], SnapshotStatus::Partial),
            (&[ExecutionResultStatus::Empty, ExecutionResultStatus::Failure], SnapshotStatus::Empty),
            (
                &[ExecutionResultStatus::Success, ExecutionResultStatus::Empty, ExecutionResultStatus::Failure],
                SnapshotStatus::Partial,
            ),
        ];
        for (statuses, expected) in cases {
            for temporal in 0..1_u32 << statuses.len() {
                let mut aggregate = ExecutionAggregate::default();
                for (position, status) in statuses.iter().enumerate() {
                    if temporal & (1 << position) == 0 {
                        aggregate.push_entity(ended(*status));
                    } else {
                        aggregate.push_temporal(ended(*status));
                    }
                }
                pretty_assertions::assert_eq!(aggregate.final_status(), expected, "{statuses:?}, temporal queries at {temporal:#b}");
            }
        }
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
