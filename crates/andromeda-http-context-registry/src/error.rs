//! Exhaustive HTTP classification for Context Registry failures.

use andromeda_context_registry::error::ContextRegistryError;
use andromeda_http_extraction::{classification::classify_request_origin_error, origin::RequestOriginError};
use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
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
        ContextRegistryError::ContextUnavailable { .. } => ClassifiedReport::public(NgsiLdErrorType::LdContextNotAvailable, error.to_string()),
        ContextRegistryError::MissingSourceUrl { .. }
        | ContextRegistryError::Persistence { .. }
        | ContextRegistryError::Representation { .. }
        | ContextRegistryError::Invariant { .. } => ClassifiedReport::internal(error),
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

/// Convert a typed request-origin failure without an aggregate broker error.
pub(crate) fn origin_problem(error: &RequestOriginError, instance: &Uri) -> ProblemResponse {
    classify_request_origin_error(error).with_instance(instance.clone())
}

#[cfg(test)]
mod tests {
    use super::classify_context_registry_error;
    use andromeda_context_registry::error::ContextRegistryError;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use std::error::Error;

    #[test]
    fn focused_context_registry_categories_and_disclosure_are_stable() -> Result<(), Box<dyn Error>> {
        let identifier = "urn:ngsi-ld:Context:1".parse::<Uri>()?;
        for (error, expected) in [
            (ContextRegistryError::MissingTopLevelContext, NgsiLdErrorType::InvalidRequest),
            (ContextRegistryError::NotFound { identifier }, NgsiLdErrorType::ResourceNotFound),
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
                pretty_assertions::assert_eq!(report.detail(), "Internal error");
            }
        }
        Ok(())
    }
}
