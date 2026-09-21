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
    use super::classify_context_registry_error;
    use andromeda_context_registry::{error::ContextRegistryError, identifier::ContextIdentifier};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;

    #[test]
    fn focused_context_registry_categories_and_disclosure_are_stable() -> Result<(), ContextRegistryError> {
        let identifier = "urn:ngsi-ld:Context:1"
            .parse::<Uri>()
            .map_err(|error| ContextRegistryError::Invariant { detail: error.to_string() })?;
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
                ContextRegistryError::Persistence {
                    detail: "database host".to_string(),
                },
                NgsiLdErrorType::InternalError,
            ),
        ] {
            let report = classify_context_registry_error(&error);
            pretty_assertions::assert_eq!(report.error_type(), expected);
            if expected == NgsiLdErrorType::InternalError {
                pretty_assertions::assert_eq!(report.detail(), "internal context-registry error");
                assert!(!report.detail().contains("database host"), "the raw cause leaked: {}", report.detail());
            }
        }
        Ok(())
    }
}
