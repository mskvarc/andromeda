//! Exhaustive HTTP classification of failures owned by the posted-query
//! policy, shared by every binding that answers one.

use andromeda_failure_classification::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_posted_query::error::PostedQueryError;

/// Classify one posted-query policy failure.
#[must_use]
pub fn classify_posted_query_error(error: &PostedQueryError) -> ClassifiedReport {
    match error {
        // Clause 5.5.9.1 has a walk continue under the query and the
        // `@context` it started with: a pointer naming a query no longer held,
        // or followed under another context, is data that does not meet the
        // requirements of the operation (clause 6.3.20).
        PostedQueryError::NotFound { .. } | PostedQueryError::ContextChanged => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()),
        PostedQueryError::StoreSaturated => ClassifiedReport::store_saturated(error.to_string()),
        PostedQueryError::StoreUnavailable => ClassifiedReport::store_unavailable(error.to_string()),
        PostedQueryError::Persistence(_) | PostedQueryError::Unencodable(_) | PostedQueryError::LifetimeOutOfRange => {
            ClassifiedReport::internal(error, InternalFailureSummary::ENTITY_STORE_FAULT)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::posted_query_failure::classify_posted_query_error;
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
    use andromeda_ngsi_ld_request::pagination::query_digest::QueryDigest;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_posted_query::error::PostedQueryError;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_pointer_to_a_query_no_longer_held_or_under_another_context_is_bad_request_data() {
        for error in [
            PostedQueryError::NotFound {
                digest: QueryDigest::of(Some("type=Vehicle")),
            },
            PostedQueryError::ContextChanged,
        ] {
            let report = classify_posted_query_error(&error);
            assert_eq!(report.error_type(), NgsiLdErrorType::BadRequestData, "{error}");
            assert_eq!(report.detail(), error.to_string());
        }
    }

    #[test]
    fn a_saturated_store_is_a_retryable_503_and_a_fault_is_masked() {
        assert_eq!(
            classify_posted_query_error(&PostedQueryError::StoreSaturated).classification(),
            ProblemClassification::ServiceUnavailable
        );
        let report = classify_posted_query_error(&PostedQueryError::Persistence(PersistenceFailure::StatementRejected));
        assert_eq!(report.error_type(), NgsiLdErrorType::InternalError);
        assert_eq!(report.detail(), "the entity store could not complete the operation");
    }
}
