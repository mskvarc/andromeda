//! Snapshot execution result aggregation and status selection.

use andromeda_ngsi_ld_model::snapshot::{ExecutionResultDetails, ExecutionResultStatus, SnapshotStatus};

#[derive(Default)]
pub(crate) struct ExecutionAggregate {
    query_details: Vec<ExecutionResultDetails>,
    temporal_query_details: Vec<ExecutionResultDetails>,
    any_success: bool,
    any_success_with_results: bool,
    any_failure: bool,
}

impl ExecutionAggregate {
    pub(crate) fn push_entity(&mut self, detail: ExecutionResultDetails) {
        self.update_flags(&detail);
        self.query_details.push(detail);
    }

    pub(crate) fn push_temporal(&mut self, detail: ExecutionResultDetails) {
        self.update_flags(&detail);
        self.temporal_query_details.push(detail);
    }

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

    pub(crate) fn detail_slices(&self) -> (Option<&[ExecutionResultDetails]>, Option<&[ExecutionResultDetails]>) {
        let query_details = (!self.query_details.is_empty()).then_some(self.query_details.as_slice());
        let temporal_query_details = (!self.temporal_query_details.is_empty()).then_some(self.temporal_query_details.as_slice());
        (query_details, temporal_query_details)
    }

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

pub(crate) fn failure_detail(message: impl Into<String>) -> ExecutionResultDetails {
    ExecutionResultDetails {
        result_status: ExecutionResultStatus::Failure,
        problem_details: Some(serde_json::json!({
            "type": "https://uri.etsi.org/ngsi-ld/errors/InternalError",
            "title": "Internal Error",
            "detail": message.into(),
            "status": 500
        })),
    }
}

#[cfg(test)]
mod tests {
    use crate::executor::status::{ExecutionAggregate, failure_detail, success_detail};
    use andromeda_ngsi_ld_model::snapshot::SnapshotStatus;

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
        aggregate.push_temporal(failure_detail("unavailable source"));
        pretty_assertions::assert_eq!(aggregate.final_status(), SnapshotStatus::Empty);
    }

    #[test]
    fn usable_data_with_a_failure_is_partial() {
        let mut aggregate = ExecutionAggregate::default();
        aggregate.push_entity(success_detail(2));
        aggregate.push_temporal(failure_detail("unavailable source"));
        pretty_assertions::assert_eq!(aggregate.final_status(), SnapshotStatus::Partial);
    }

    #[test]
    fn failures_without_a_success_are_failure() {
        let mut aggregate = ExecutionAggregate::default();
        aggregate.push_entity(failure_detail("invalid query"));
        pretty_assertions::assert_eq!(aggregate.final_status(), SnapshotStatus::Failure);
    }

    #[test]
    fn no_executions_are_failure() {
        pretty_assertions::assert_eq!(ExecutionAggregate::default().final_status(), SnapshotStatus::Failure);
    }
}
