//! Exhaustive HTTP classification for Context Registry failures.

use andromeda_context_registry::error::ContextRegistryError;
use andromeda_http_response::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary, problem_response::ProblemResponse};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use axum::http::Uri;

/// Classify one Context Registry policy failure for its HTTP surface.
#[must_use]
pub fn classify_context_registry_error(error: &ContextRegistryError) -> ClassifiedReport {
    match error {
        ContextRegistryError::MissingTopLevelContext => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, error.to_string()),
        ContextRegistryError::ReloadRequiresCached { .. } | ContextRegistryError::CoreContextDeletion => {
            ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string())
        }
        ContextRegistryError::NotFound { .. } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, error.to_string()),
        // Clause 5.13.4.4: serving the full content of a `Cached` @context
        // "shall give back OperationNotSupported".
        ContextRegistryError::CachedContextNotServed { .. } => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, error.to_string()),
        ContextRegistryError::ContextUnavailable { .. } => ClassifiedReport::public(NgsiLdErrorType::LdContextNotAvailable, error.to_string()),
        // Backpressure, not a fault: shed as 503 with `Retry-After`, not a 500.
        ContextRegistryError::StoreSaturated => ClassifiedReport::store_saturated(error.to_string()),
        ContextRegistryError::StoreUnavailable => ClassifiedReport::store_unavailable(error.to_string()),
        ContextRegistryError::MissingSourceUrl { .. }
        | ContextRegistryError::Persistence { .. }
        | ContextRegistryError::Representation { .. }
        | ContextRegistryError::Invariant { .. } => ClassifiedReport::internal(error, InternalFailureSummary::new("internal context-registry error")),
    }
}

/// Attach the request URI while converting a typed Context Registry result.
pub trait ContextRegistryResultExt<T> {
    /// Map the focused error and attach `instance`.
    ///
    /// # Errors
    ///
    /// Returns the [`ProblemResponse`] the failure classifies to whenever `self`
    /// is `Err`; an `Ok` result passes through untouched.
    fn with_context_registry_instance(self, instance: &Uri) -> Result<T, ProblemResponse>;
}

impl<T> ContextRegistryResultExt<T> for Result<T, ContextRegistryError> {
    fn with_context_registry_instance(self, instance: &Uri) -> Result<T, ProblemResponse> {
        self.map_err(|error| classify_context_registry_error(&error).with_instance(instance.clone()))
    }
}

#[cfg(test)]
mod tests {
    use crate::error::classify_context_registry_error;
    use andromeda_context_registry::{error::ContextRegistryError, identifier::ContextIdentifier};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;

    #[test]
    fn focused_context_registry_categories_and_disclosure_are_stable() {
        let identifier = "urn:ngsi-ld:JsonLdContext:1".parse::<Identifier>().expect("a valid test identifier");
        for (error, expected) in [
            (ContextRegistryError::MissingTopLevelContext, NgsiLdErrorType::InvalidRequest),
            (
                ContextRegistryError::NotFound {
                    identifier: ContextIdentifier::from(identifier),
                },
                NgsiLdErrorType::ResourceNotFound,
            ),
            (ContextRegistryError::CoreContextDeletion, NgsiLdErrorType::BadRequestData),
            (
                ContextRegistryError::Persistence(PersistenceFailure::StatementRejected),
                NgsiLdErrorType::InternalError,
            ),
        ] {
            let report = classify_context_registry_error(&error);
            pretty_assertions::assert_eq!(report.error_type(), expected);
            if expected == NgsiLdErrorType::InternalError {
                pretty_assertions::assert_eq!(report.detail(), "internal context-registry error");
                assert!(
                    !report.detail().contains(&PersistenceFailure::StatementRejected.to_string()),
                    "the raw cause leaked: {}",
                    report.detail()
                );
            }
        }
    }

    /// Clause 5.5.6 with RFC 7231 6.6.4: a store that cannot be reached is a
    /// retryable 503 with `Retry-After`, never the 500 of a fault.
    #[test]
    fn an_unreachable_store_is_a_retryable_503() {
        use andromeda_ngsi_ld_problem_details::problem_detail::ProblemClassification;
        use std::time::Duration;

        let report = classify_context_registry_error(&ContextRegistryError::StoreUnavailable);
        pretty_assertions::assert_eq!(report.classification(), ProblemClassification::ServiceUnavailable);
        pretty_assertions::assert_eq!(report.retry_after(), Some(Duration::from_secs(5)));
        pretty_assertions::assert_eq!(report.detail(), "the datastore is temporarily unavailable; retry shortly");
    }
}
