//! Classifying a store failure carried by value rather than converted into a
//! port's own retryable categories.

use crate::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use std::fmt::Display;

/// Classify the store failure `failure` that `error` carries.
///
/// The three retryable conditions are answered 503 with `Retry-After` and
/// `error`'s own detail, which names the condition and no backend text: a
/// saturated store, a transaction the store kept rolling back, and a store
/// that cannot be reached. Every fault is logged with `error` and masked to
/// `summary` (clause 5.5.6).
pub fn classify_store_failure(failure: PersistenceFailure, error: &impl Display, summary: InternalFailureSummary) -> ClassifiedReport {
    match failure {
        PersistenceFailure::Saturated => ClassifiedReport::store_saturated(error.to_string()),
        PersistenceFailure::RolledBack => ClassifiedReport::transaction_contended(error.to_string()),
        PersistenceFailure::Unavailable => ClassifiedReport::store_unavailable(error.to_string()),
        PersistenceFailure::Misconfigured
        | PersistenceFailure::ConstraintViolated
        | PersistenceFailure::StatementRejected
        | PersistenceFailure::RecordMissing
        | PersistenceFailure::Undecodable
        | PersistenceFailure::Unencodable
        | PersistenceFailure::StatementUnbuildable
        | PersistenceFailure::SchemaMismatch
        | PersistenceFailure::ContractBroken => ClassifiedReport::internal(error, summary),
    }
}

#[cfg(test)]
mod tests {
    use crate::{internal_failure_summary::InternalFailureSummary, store_failure::classify_store_failure};
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use std::time::Duration;

    const SUMMARY: InternalFailureSummary = InternalFailureSummary::new("the store failed");

    #[test]
    fn each_retryable_condition_is_a_503_with_its_own_retry_after() {
        for (failure, retry_after) in [
            (PersistenceFailure::Saturated, Duration::from_secs(1)),
            (PersistenceFailure::RolledBack, Duration::from_secs(1)),
            (PersistenceFailure::Unavailable, Duration::from_secs(5)),
        ] {
            let report = classify_store_failure(failure, &failure, SUMMARY);
            pretty_assertions::assert_eq!(report.classification(), ProblemClassification::ServiceUnavailable, "{failure}");
            pretty_assertions::assert_eq!(report.retry_after(), Some(retry_after), "{failure}");
            pretty_assertions::assert_eq!(report.detail(), failure.to_string());
        }
    }

    #[test]
    fn a_fault_is_masked_to_the_summary() {
        for failure in [PersistenceFailure::Misconfigured, PersistenceFailure::Undecodable] {
            let report = classify_store_failure(failure, &failure, SUMMARY);
            pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::InternalError, "{failure}");
            pretty_assertions::assert_eq!(report.detail(), "the store failed");
            pretty_assertions::assert_eq!(report.retry_after(), None);
        }
    }
}
