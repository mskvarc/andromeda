//! Exhaustive HTTP classification of failures owned by the Entity Map policy,
//! shared by every binding that answers one.

use crate::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary};
use andromeda_entity_map::error::EntityMapError;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;

/// Classify one Entity Map policy failure.
#[must_use]
pub fn classify_entity_map_error(error: &EntityMapError) -> ClassifiedReport {
    match error {
        EntityMapError::NotFound { .. } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, error.to_string()),
        // Clause 5.5.14 with clause 6.3.20: parameters incompatible with
        // the request that created the map are an invalid request.
        EntityMapError::RequestChanged { .. } => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, error.to_string()),
        EntityMapError::StoreSaturated => ClassifiedReport::store_saturated(error.to_string()),
        EntityMapError::StoreUnavailable => ClassifiedReport::store_unavailable(error.to_string()),
        EntityMapError::Persistence(_) | EntityMapError::Invariant(_) => ClassifiedReport::internal(error, InternalFailureSummary::ENTITY_STORE_FAULT),
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_map_failure::classify_entity_map_error;
    use andromeda_entity_map::error::EntityMapError;
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use std::time::Duration;

    #[test]
    fn a_saturated_or_unreachable_map_store_is_a_retryable_503() {
        for (error, retry_after) in [
            (EntityMapError::StoreSaturated, Duration::from_secs(1)),
            (EntityMapError::StoreUnavailable, Duration::from_secs(5)),
        ] {
            let report = classify_entity_map_error(&error);
            pretty_assertions::assert_eq!(report.classification(), ProblemClassification::ServiceUnavailable, "{error}");
            pretty_assertions::assert_eq!(report.retry_after(), Some(retry_after), "{error}");
        }
    }

    #[test]
    fn a_map_store_fault_is_masked_as_an_internal_error() {
        let report = classify_entity_map_error(&EntityMapError::Persistence(PersistenceFailure::StatementRejected));
        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::InternalError);
        pretty_assertions::assert_eq!(report.detail(), "the entity store could not complete the operation");
        pretty_assertions::assert_eq!(report.retry_after(), None);
    }
}
