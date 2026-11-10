//! How one Snapshot query's capture ended, beside what it left in the
//! Snapshot's staging branch (clause 5.16.1.4).
//!
//! A query that fails as clause 5.7.2.4 or 5.7.4.4 describes retrieves no
//! Entity information, so nothing of it may stay in the Snapshot. A capture
//! stores each page as it reads it, so a query that fails after its first write
//! has part of its result in the staging branch. That part cannot be taken out
//! on its own: another query of the Snapshot may have captured the same
//! entities, and the history of one entity holds what several queries wrote.

use andromeda_ngsi_ld_model::snapshot::{ExecutionResultDetails, ExecutionResultStatus};

/// Whether a query handed any of its result to the staging branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Staging {
    /// No write of the query reached the branch.
    #[default]
    Untouched,
    /// A write of the query was sent to the branch, whether or not it held.
    Written,
}

/// How one query's capture ended.
#[derive(Debug)]
pub(super) enum QueryCapture {
    /// The query's whole result is staged, or the query failed before any of
    /// it was written.
    Settled(ExecutionResultDetails),
    /// The query failed after part of its result was written into the staging
    /// branch.
    FailedAfterStaging(ExecutionResultDetails),
}

impl QueryCapture {
    /// How a query whose execution result is `detail` ended, `staging` saying
    /// whether it wrote into the staging branch.
    pub(super) const fn of(detail: ExecutionResultDetails, staging: Staging) -> QueryCapture {
        match (&detail.result_status, staging) {
            (ExecutionResultStatus::Failure, Staging::Written) => QueryCapture::FailedAfterStaging(detail),
            (ExecutionResultStatus::Failure, Staging::Untouched) | (ExecutionResultStatus::Success | ExecutionResultStatus::Empty, _) => {
                QueryCapture::Settled(detail)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::executor::{
        execution_failure::ExecutionFailure,
        query_capture::{QueryCapture, Staging},
        status::{failure_detail, success_detail},
    };

    #[test]
    fn a_query_that_failed_after_writing_failed_after_staging() {
        let detail = failure_detail(ExecutionFailure::OffsetOverflow.problem());

        assert!(matches!(QueryCapture::of(detail, Staging::Written), QueryCapture::FailedAfterStaging(_)));
    }

    #[test]
    fn a_query_that_failed_before_writing_is_settled() {
        let detail = failure_detail(ExecutionFailure::OffsetOverflow.problem());

        assert!(matches!(QueryCapture::of(detail, Staging::Untouched), QueryCapture::Settled(_)));
    }

    #[test]
    fn a_query_that_succeeded_is_settled_whatever_it_wrote() {
        for staging in [Staging::Untouched, Staging::Written] {
            assert!(matches!(QueryCapture::of(success_detail(1), staging), QueryCapture::Settled(_)));
            assert!(matches!(QueryCapture::of(success_detail(0), staging), QueryCapture::Settled(_)));
        }
    }
}
