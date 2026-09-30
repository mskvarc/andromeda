//! Exhaustive HTTP classification for subscription failures.

use andromeda_http_extraction::classification::{classify_jsonld_error, classify_parameter_error, classify_query_error};
use andromeda_http_response::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary, problem_response::ProblemResponse};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_render::error::RenderError;
use andromeda_ngsi_ld_request::error::ParameterError;
use andromeda_subscription::error::SubscriptionError;
use axum::http::Uri;
use std::fmt::Display;

/// A typed failure consumed by subscription routes.
pub trait SubscriptionHttpClassify {
    /// Select the NGSI-LD category and disclosure.
    fn classify_for_subscription_http(&self) -> ClassifiedReport;
}

impl SubscriptionHttpClassify for SubscriptionError {
    fn classify_for_subscription_http(&self) -> ClassifiedReport {
        match self {
            SubscriptionError::Validation(reason) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, reason.to_string()),
            SubscriptionError::NotFound { .. } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, self.to_string()),
            SubscriptionError::AlreadyExists { .. } => ClassifiedReport::public(NgsiLdErrorType::AlreadyExists, self.to_string()),
            SubscriptionError::JsonLd(source) => classify_jsonld_error(source),
            SubscriptionError::Query(source) => classify_query_error(source),
            // Backpressure, not a fault: shed as 503 with `Retry-After`, not a 500.
            SubscriptionError::StoreSaturated => ClassifiedReport::store_saturated(self.to_string()),
            SubscriptionError::Persistence(_) => ClassifiedReport::internal(self, InternalFailureSummary::new("the subscription store is unavailable")),
            SubscriptionError::Representation(_) | SubscriptionError::Identifier(_) | SubscriptionError::Internal(_) => {
                ClassifiedReport::internal(self, InternalFailureSummary::new("internal subscription error"))
            }
        }
    }
}

// Clause 6.3.20 separates an unsupported parameter value from invalid data,
// and `classify_parameter_error` is where that distinction is already drawn.
impl SubscriptionHttpClassify for ParameterError {
    fn classify_for_subscription_http(&self) -> ClassifiedReport {
        classify_parameter_error(self)
    }
}

impl SubscriptionHttpClassify for RenderError {
    fn classify_for_subscription_http(&self) -> ClassifiedReport {
        ClassifiedReport::internal(self, InternalFailureSummary::RESPONSE_RENDERING)
    }
}

/// Attach the inbound URI while classifying a typed subscription-binding result.
pub trait SubscriptionResultExt<T> {
    /// Map the focused failure and attach `instance`.
    ///
    /// # Errors
    ///
    /// Returns a [`ProblemResponse`] whenever the receiver is `Err`: the typed
    /// failure is classified into its NGSI-LD `ProblemDetails` category and the
    /// inbound request URI is recorded as the `instance` member.
    fn with_subscription_instance(self, instance: &Uri) -> Result<T, ProblemResponse>;
}

impl<T, E> SubscriptionResultExt<T> for Result<T, E>
where
    E: SubscriptionHttpClassify,
{
    fn with_subscription_instance(self, instance: &Uri) -> Result<T, ProblemResponse> {
        self.map_err(|error| error.classify_for_subscription_http().with_instance(instance.clone()))
    }
}

pub(crate) fn public_problem(detail: impl Into<String>, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::public(NgsiLdErrorType::BadRequestData, detail).with_instance(instance.clone())
}

/// Sanitize a failure that no client input can provoke, attaching `instance`.
pub(crate) fn internal_problem(error: &impl Display, summary: InternalFailureSummary, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::internal(error, summary).with_instance(instance.clone())
}

#[cfg(test)]
mod tests {
    use super::SubscriptionHttpClassify;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_subscription::{error::SubscriptionError, subscription_validation_reason::SubscriptionValidationReason};
    use std::error::Error;

    #[test]
    fn focused_subscription_categories_and_disclosure_are_stable() -> Result<(), Box<dyn Error>> {
        let identifier = "urn:ngsi-ld:Subscription:1".parse::<Identifier>()?;
        for (error, expected) in [
            (
                SubscriptionError::Validation(SubscriptionValidationReason::NotificationRequired),
                NgsiLdErrorType::BadRequestData,
            ),
            (
                SubscriptionError::NotFound {
                    identifier: identifier.clone(),
                },
                NgsiLdErrorType::ResourceNotFound,
            ),
            (SubscriptionError::AlreadyExists { identifier }, NgsiLdErrorType::AlreadyExists),
            (SubscriptionError::Persistence(PersistenceFailure::Unavailable), NgsiLdErrorType::InternalError),
        ] {
            let report = error.classify_for_subscription_http();
            pretty_assertions::assert_eq!(report.error_type(), expected);
            if expected == NgsiLdErrorType::InternalError {
                pretty_assertions::assert_eq!(report.detail(), "the subscription store is unavailable");
                assert!(
                    !report.detail().contains(&PersistenceFailure::Unavailable.to_string()),
                    "the raw cause leaked: {}",
                    report.detail()
                );
            }
        }
        Ok(())
    }
}
