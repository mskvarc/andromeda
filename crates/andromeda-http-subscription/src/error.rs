//! Exhaustive HTTP classification for subscription failures.

use andromeda_http_extraction::{
    classification::{classify_jsonld_error, classify_query_error, classify_request_origin_error},
    origin::RequestOriginError,
};
use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_render::error::RenderError;
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
            SubscriptionError::Validation { detail } => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, detail),
            SubscriptionError::NotFound { .. } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, self.to_string()),
            SubscriptionError::AlreadyExists { .. } => ClassifiedReport::public(NgsiLdErrorType::AlreadyExists, self.to_string()),
            SubscriptionError::JsonLd(source) => classify_jsonld_error(source),
            SubscriptionError::Query(source) => classify_query_error(source),
            SubscriptionError::Representation(_)
            | SubscriptionError::Identifier(_)
            | SubscriptionError::Persistence { .. }
            | SubscriptionError::Internal { .. } => ClassifiedReport::internal(self),
        }
    }
}

impl SubscriptionHttpClassify for RenderError {
    fn classify_for_subscription_http(&self) -> ClassifiedReport {
        ClassifiedReport::internal(self)
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

pub(crate) fn origin_problem(error: &RequestOriginError, instance: &Uri) -> ProblemResponse {
    classify_request_origin_error(error).with_instance(instance.clone())
}

/// Sanitize a failure that no client input can provoke, attaching `instance`.
pub(crate) fn internal_problem(error: &impl Display, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::internal(error).with_instance(instance.clone())
}

#[cfg(test)]
mod tests {
    use super::SubscriptionHttpClassify;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_subscription::error::SubscriptionError;
    use std::error::Error;

    #[test]
    fn focused_subscription_categories_and_disclosure_are_stable() -> Result<(), Box<dyn Error>> {
        let identifier = "urn:ngsi-ld:Subscription:1".parse::<Uri>()?;
        for (error, expected) in [
            (SubscriptionError::Validation { detail: "invalid".to_string() }, NgsiLdErrorType::BadRequestData),
            (
                SubscriptionError::NotFound {
                    identifier: identifier.clone(),
                },
                NgsiLdErrorType::ResourceNotFound,
            ),
            (SubscriptionError::AlreadyExists { identifier }, NgsiLdErrorType::AlreadyExists),
            (
                SubscriptionError::Persistence {
                    detail: "database host".to_string(),
                },
                NgsiLdErrorType::InternalError,
            ),
        ] {
            let report = error.classify_for_subscription_http();
            pretty_assertions::assert_eq!(report.error_type(), expected);
            if expected == NgsiLdErrorType::InternalError {
                pretty_assertions::assert_eq!(report.detail(), "Internal error");
            }
        }
        Ok(())
    }
}
