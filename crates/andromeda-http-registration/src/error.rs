//! Exhaustive HTTP classification for context-source registration failures.

use andromeda_failure_classification::{
    classified_report::ClassifiedReport,
    internal_failure_summary::InternalFailureSummary,
    jsonld_failure::classify_jsonld_error,
    parameter_failure::classify_parameter_error,
    query_failure::classify_query_error,
};
use andromeda_http_context_registry::response_rendering::{ResponseContextError, classify_response_context_error};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_render::error::RenderError;
use andromeda_ngsi_ld_request::error::ParameterError;
use andromeda_registration::error::RegistrationError;
use axum::http::Uri;
use std::{fmt::Display, num::TryFromIntError};
use thiserror::Error;

/// A stored registration count the response cannot carry.
#[derive(Debug, Error)]
pub(crate) enum RegistrationCountError {
    /// The count does not fit the platform's `usize`.
    #[error("registration count does not fit the response platform: {0}")]
    ResponsePlatform(#[source] TryFromIntError),
    /// The count does not fit the signed offset the pagination arithmetic uses.
    #[error("registration count does not fit offset pagination: {0}")]
    OffsetPagination(#[source] TryFromIntError),
}

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
            RegistrationError::StoreUnavailable => ClassifiedReport::store_unavailable(self.to_string()),
            RegistrationError::Persistence(_) => ClassifiedReport::internal(self, InternalFailureSummary::REGISTRATION_STORE_FAULT),
            RegistrationError::Internal(_) => ClassifiedReport::internal(self, InternalFailureSummary::new("internal registration error")),
        }
    }
}

// Clause 6.3.20 separates an unsupported parameter value from invalid data;
// `classify_parameter_error` draws that distinction.
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
        self.map_err(|error| ProblemResponse::classified(error.classify_for_registration_http(), instance.clone()))
    }
}

/// Sanitize a binding invariant and attach the request URI.
pub(crate) fn internal_problem(error: impl Display, summary: InternalFailureSummary, instance: &Uri) -> ProblemResponse {
    ProblemResponse::classified(ClassifiedReport::internal(&error, summary), instance.clone())
}

#[cfg(test)]
mod tests {
    use crate::error::RegistrationHttpClassify;
    use andromeda_ngsi_ld_geometry::geometry_census::{GeometryOverLimit, GeometryPositionLimit};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
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
                RegistrationError::Validation(RegistrationValidationReason::MemberRequired { member: CsrMember::Endpoint }),
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
            (
                RegistrationError::Persistence(PersistenceFailure::StatementRejected),
                NgsiLdErrorType::InternalError,
            ),
        ] {
            let report = error.classify_for_registration_http();
            pretty_assertions::assert_eq!(report.error_type(), expected);
            if expected == NgsiLdErrorType::InternalError {
                pretty_assertions::assert_eq!(report.detail(), "the registration store could not complete the operation");
                assert!(
                    !report.detail().contains(&PersistenceFailure::StatementRejected.to_string()),
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

    /// Clause 5.5.6 with RFC 7231 6.6.4: a store that cannot be reached is a
    /// retryable 503 with `Retry-After`, never the 500 of a fault.
    #[test]
    fn an_unreachable_store_is_a_retryable_503() {
        use andromeda_ngsi_ld_problem_details::problem_detail::ProblemClassification;
        use std::time::Duration;

        let report = RegistrationError::StoreUnavailable.classify_for_registration_http();
        pretty_assertions::assert_eq!(report.classification(), ProblemClassification::ServiceUnavailable);
        pretty_assertions::assert_eq!(report.retry_after(), Some(Duration::from_secs(5)));
        pretty_assertions::assert_eq!(report.detail(), "the datastore is temporarily unavailable; retry shortly");
    }
}
