//! Exhaustive HTTP classification for context-source registration failures.

use andromeda_http_extraction::classification::{classify_jsonld_error, classify_query_error};
use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_render::error::RenderError;
use andromeda_registration::error::RegistrationError;
use axum::http::Uri;
use std::fmt::Display;

/// A typed failure consumed by the registration HTTP binding.
pub trait RegistrationHttpClassify {
    /// Select the NGSI-LD category and disclosure for this failure.
    fn classify_for_registration_http(&self) -> ClassifiedReport;
}

impl RegistrationHttpClassify for RegistrationError {
    fn classify_for_registration_http(&self) -> ClassifiedReport {
        match self {
            RegistrationError::Validation { detail } => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, detail),
            RegistrationError::NotFound { .. } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, self.to_string()),
            RegistrationError::AlreadyExists { .. } => ClassifiedReport::public(NgsiLdErrorType::AlreadyExists, self.to_string()),
            RegistrationError::Conflict { detail } => ClassifiedReport::public(NgsiLdErrorType::Conflict, detail),
            RegistrationError::JsonLd(source) => classify_jsonld_error(source),
            RegistrationError::Query(source) => classify_query_error(source),
            RegistrationError::Representation(_)
            | RegistrationError::Identifier(_)
            | RegistrationError::Persistence { .. }
            | RegistrationError::Internal { .. } => ClassifiedReport::internal(self),
        }
    }
}

impl RegistrationHttpClassify for RenderError {
    fn classify_for_registration_http(&self) -> ClassifiedReport {
        ClassifiedReport::internal(self)
    }
}

/// Attach the inbound URI while classifying a typed registration-binding result.
pub trait RegistrationResultExt<T> {
    /// Map the focused failure and attach `instance`.
    fn with_registration_instance(self, instance: &Uri) -> Result<T, ProblemResponse>;
}

impl<T, E> RegistrationResultExt<T> for Result<T, E>
where
    E: RegistrationHttpClassify,
{
    fn with_registration_instance(self, instance: &Uri) -> Result<T, ProblemResponse> {
        self.map_err(|error| error.classify_for_registration_http().with_instance(instance.clone()))
    }
}

/// Sanitize a binding invariant and attach the request URI.
pub(crate) fn internal_problem(detail: impl Display, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::internal(&detail).with_instance(instance.clone())
}

#[cfg(test)]
mod tests {
    use super::RegistrationHttpClassify;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_registration::error::RegistrationError;
    use std::error::Error;

    #[test]
    fn focused_registration_categories_and_disclosure_are_stable() -> Result<(), Box<dyn Error>> {
        let identifier = "urn:ngsi-ld:ContextSourceRegistration:1".parse::<Uri>()?;
        for (error, expected) in [
            (RegistrationError::Validation { detail: "invalid".to_string() }, NgsiLdErrorType::BadRequestData),
            (
                RegistrationError::NotFound {
                    identifier: identifier.clone(),
                },
                NgsiLdErrorType::ResourceNotFound,
            ),
            (RegistrationError::AlreadyExists { identifier }, NgsiLdErrorType::AlreadyExists),
            (RegistrationError::Conflict { detail: "overlap".to_string() }, NgsiLdErrorType::Conflict),
            (
                RegistrationError::Persistence {
                    detail: "database host".to_string(),
                },
                NgsiLdErrorType::InternalError,
            ),
        ] {
            let report = error.classify_for_registration_http();
            pretty_assertions::assert_eq!(report.error_type(), expected);
            if expected == NgsiLdErrorType::InternalError {
                pretty_assertions::assert_eq!(report.detail(), "Internal error");
            }
        }
        Ok(())
    }
}
