//! Exhaustive HTTP classification for context-source registration failures.

use andromeda_http_context_registry::response_rendering::{ResponseContextError, classify_response_context_error};
use andromeda_http_extraction::classification::{classify_jsonld_error, classify_parameter_error, classify_query_error};
use andromeda_http_response::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary, problem_response::ProblemResponse};
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_render::error::RenderError;
use andromeda_ngsi_ld_request::error::ParameterError;
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
            RegistrationError::Validation(reason) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, reason.to_string()),
            RegistrationError::NotFound { .. } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, self.to_string()),
            RegistrationError::AlreadyExists { .. } => ClassifiedReport::public(NgsiLdErrorType::AlreadyExists, self.to_string()),
            RegistrationError::Conflict(reason) => ClassifiedReport::public(NgsiLdErrorType::Conflict, reason.to_string()),
            // Clause 6.3.2: a written input larger than the broker processes
            // is "Request Entity too large", counted here in positions.
            RegistrationError::GeometryTooLarge { .. } => ClassifiedReport::public(NgsiLdErrorType::RequestEntityTooLarge, self.to_string()),
            RegistrationError::JsonLd(source) => classify_jsonld_error(source),
            RegistrationError::Query(source) => classify_query_error(source),
            // Backpressure, not a fault: shed as 503 with `Retry-After`, not a 500.
            RegistrationError::StoreSaturated => ClassifiedReport::store_saturated(self.to_string()),
            RegistrationError::Persistence(_) => ClassifiedReport::internal(self, InternalFailureSummary::new("the registration store is unavailable")),
            RegistrationError::Representation(_) | RegistrationError::Internal(_) => {
                ClassifiedReport::internal(self, InternalFailureSummary::new("internal registration error"))
            }
        }
    }
}

// Clause 6.3.20 separates an unsupported parameter value from invalid data,
// and `classify_parameter_error` is where that distinction is already drawn.
impl RegistrationHttpClassify for ParameterError {
    fn classify_for_registration_http(&self) -> ClassifiedReport {
        classify_parameter_error(self)
    }
}

impl RegistrationHttpClassify for JsonLdError {
    fn classify_for_registration_http(&self) -> ClassifiedReport {
        classify_jsonld_error(self)
    }
}

impl RegistrationHttpClassify for ResponseContextError {
    fn classify_for_registration_http(&self) -> ClassifiedReport {
        classify_response_context_error(self)
    }
}

impl RegistrationHttpClassify for RenderError {
    fn classify_for_registration_http(&self) -> ClassifiedReport {
        ClassifiedReport::internal(self, InternalFailureSummary::RESPONSE_RENDERING)
    }
}

/// Attach the inbound URI while classifying a typed registration-binding result.
pub trait RegistrationResultExt<T> {
    /// Map the focused failure and attach `instance`.
    ///
    /// # Errors
    ///
    /// Returns the [`ProblemResponse`] the failure classifies to whenever `self`
    /// is `Err`; an `Ok` result passes through untouched.
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
pub(crate) fn internal_problem(detail: impl Display, summary: InternalFailureSummary, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::internal(&detail, summary).with_instance(instance.clone())
}

#[cfg(test)]
mod tests {
    use crate::error::RegistrationHttpClassify;
    use andromeda_ngsi_ld_geometry::geometry_census::{GeometryOverLimit, GeometryPositionLimit};
    use andromeda_ngsi_ld_lexicon::{
        identifier::{Identifier, IdentifierParseError},
        uri::UriParseError,
    };
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_registration::{
        csr_member::CsrMember,
        error::RegistrationError,
        registration_conflict_reason::RegistrationConflictReason,
        registration_validation_reason::RegistrationValidationReason,
    };
    use axum::http::StatusCode;
    use std::{error::Error, num::NonZeroUsize};

    #[test]
    fn focused_registration_categories_and_disclosure_are_stable() -> Result<(), Box<dyn Error>> {
        let identifier = "urn:ngsi-ld:ContextSourceRegistration:1".parse::<Identifier>()?;
        for (error, expected) in [
            (
                RegistrationError::Validation(RegistrationValidationReason::IdNotUri {
                    source: IdentifierParseError::Malformed(UriParseError),
                }),
                NgsiLdErrorType::BadRequestData,
            ),
            (
                RegistrationError::NotFound {
                    identifier: identifier.clone(),
                },
                NgsiLdErrorType::ResourceNotFound,
            ),
            (RegistrationError::AlreadyExists { identifier }, NgsiLdErrorType::AlreadyExists),
            (
                RegistrationError::Conflict(RegistrationConflictReason::LocalEntityMatchesRedirect),
                NgsiLdErrorType::Conflict,
            ),
            (RegistrationError::Persistence(PersistenceFailure::Unavailable), NgsiLdErrorType::InternalError),
        ] {
            let report = error.classify_for_registration_http();
            pretty_assertions::assert_eq!(report.error_type(), expected);
            if expected == NgsiLdErrorType::InternalError {
                pretty_assertions::assert_eq!(report.detail(), "the registration store is unavailable");
                assert!(
                    !report.detail().contains(&PersistenceFailure::Unavailable.to_string()),
                    "the raw cause leaked: {}",
                    report.detail()
                );
            }
        }
        Ok(())
    }

    /// Clause 6.3.2: a registration geometry holding more positions than the
    /// broker accepts in one geometry is the 413 "Request Entity too large",
    /// naming the member and the limit.
    #[test]
    fn a_registration_location_past_the_position_budget_is_a_413() {
        let one_million = NonZeroUsize::new(1_000_000).expect("one million is not zero");
        let error = RegistrationError::GeometryTooLarge {
            member: CsrMember::Location,
            over: GeometryOverLimit::Positions {
                limit: GeometryPositionLimit::new(one_million),
            },
        };

        let report = error.classify_for_registration_http();

        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::RequestEntityTooLarge);
        pretty_assertions::assert_eq!(report.classification().status_code(), StatusCode::PAYLOAD_TOO_LARGE);
        assert!(report.detail().contains("location"), "{}", report.detail());
        assert!(report.detail().contains("1000000"), "{}", report.detail());
    }
}
