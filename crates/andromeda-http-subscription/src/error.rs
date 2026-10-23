//! Exhaustive HTTP classification for subscription failures.

use andromeda_http_context_registry::response_rendering::{ResponseContextError, classify_response_context_error};
use andromeda_http_response::{
    classified_report::ClassifiedReport,
    context_hosting_failure::classify_context_hosting_error,
    internal_failure_summary::InternalFailureSummary,
    jsonld_failure::classify_jsonld_error,
    parameter_failure::classify_parameter_error,
    problem_response::ProblemResponse,
    query_failure::classify_query_error,
};
use andromeda_jsonld::error::JsonLdError;
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
            // Clause 6.3.2: a written input larger than the broker processes
            // is "Request Entity too large", counted here in positions.
            SubscriptionError::GeoQTooLarge { .. } => ClassifiedReport::public(NgsiLdErrorType::RequestEntityTooLarge, self.to_string()),
            SubscriptionError::JsonLd(source) => classify_jsonld_error(source),
            SubscriptionError::ContextHosting(source) => classify_context_hosting_error(source),
            SubscriptionError::Query(source) => classify_query_error(source),
            // Backpressure, not a fault: shed as 503 with `Retry-After`, not a 500.
            SubscriptionError::StoreSaturated => ClassifiedReport::store_saturated(self.to_string()),
            SubscriptionError::StoreUnavailable => ClassifiedReport::store_unavailable(self.to_string()),
            SubscriptionError::Persistence(_) => ClassifiedReport::internal(self, InternalFailureSummary::SUBSCRIPTION_STORE_FAULT),
            SubscriptionError::Representation(_) | SubscriptionError::Internal(_) => {
                ClassifiedReport::internal(self, InternalFailureSummary::new("internal subscription error"))
            }
        }
    }
}

// Clause 6.3.20 separates an unsupported parameter value from invalid data;
// `classify_parameter_error` draws that distinction.
impl SubscriptionHttpClassify for ParameterError {
    fn classify_for_subscription_http(&self) -> ClassifiedReport {
        classify_parameter_error(self)
    }
}

impl SubscriptionHttpClassify for JsonLdError {
    fn classify_for_subscription_http(&self) -> ClassifiedReport {
        classify_jsonld_error(self)
    }
}

impl SubscriptionHttpClassify for ResponseContextError {
    fn classify_for_subscription_http(&self) -> ClassifiedReport {
        classify_response_context_error(self)
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
    use crate::error::SubscriptionHttpClassify;
    use andromeda_jsonld::compound_context_host::ContextHostingError;
    use andromeda_ngsi_ld_geometry::geometry_census::{GeometryOverLimit, GeometryPositionLimit};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_subscription::{error::SubscriptionError, subscription_validation_reason::SubscriptionValidationReason};
    use axum::{
        http::{StatusCode, Uri, header::RETRY_AFTER},
        response::IntoResponse,
    };
    use std::{error::Error, num::NonZeroUsize};

    #[test]
    fn focused_subscription_categories_and_disclosure_are_stable() -> Result<(), Box<dyn Error>> {
        let identifier = "urn:ngsi-ld:Subscription:1".parse::<Identifier>()?;
        for (error, expected) in [
            (
                SubscriptionError::Validation(SubscriptionValidationReason::TypeRequired),
                NgsiLdErrorType::BadRequestData,
            ),
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
            (
                SubscriptionError::Persistence(PersistenceFailure::StatementRejected),
                NgsiLdErrorType::InternalError,
            ),
        ] {
            let report = error.classify_for_subscription_http();
            pretty_assertions::assert_eq!(report.error_type(), expected);
            if expected == NgsiLdErrorType::InternalError {
                pretty_assertions::assert_eq!(report.detail(), "the subscription store could not complete the operation");
                assert!(
                    !report.detail().contains(&PersistenceFailure::StatementRejected.to_string()),
                    "the raw cause leaked: {}",
                    report.detail()
                );
            }
        }
        Ok(())
    }

    /// A saturated store of hosted `@context`s is backpressure: 503 with
    /// `Retry-After`, as every saturated store is.
    #[test]
    fn a_saturated_context_host_is_service_unavailable_with_retry_after() {
        let error = SubscriptionError::ContextHosting(ContextHostingError::Persistence(PersistenceFailure::Saturated));

        let response = error
            .classify_for_subscription_http()
            .with_instance(Uri::from_static("/ngsi-ld/v1/subscriptions"))
            .into_response();

        pretty_assertions::assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(response.headers().contains_key(RETRY_AFTER));
    }

    /// A missing host and a failed store are faults no client can act on:
    /// 500 `InternalError`, with the cause kept from the detail.
    #[test]
    fn a_missing_or_failed_context_host_is_an_internal_error_without_its_cause() {
        for error in [
            ContextHostingError::NoHost,
            ContextHostingError::Persistence(PersistenceFailure::StatementRejected),
        ] {
            let cause = error.to_string();
            let report = SubscriptionError::ContextHosting(error).classify_for_subscription_http();
            pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::InternalError);
            assert!(!report.detail().contains(&cause), "the raw cause leaked: {}", report.detail());
        }
    }

    /// Clause 6.3.2: a `geoQ` geometry holding more positions than the broker
    /// accepts in one geometry is the 413 "Request Entity too large", naming
    /// the count and the limit.
    #[test]
    fn a_subscription_geo_q_past_the_position_budget_is_a_413() {
        let one_million = NonZeroUsize::new(1_000_000).expect("one million is not zero");
        let error = SubscriptionError::GeoQTooLarge {
            over: GeometryOverLimit::Positions {
                limit: GeometryPositionLimit::new(one_million),
            },
        };

        let report = error.classify_for_subscription_http();

        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::RequestEntityTooLarge);
        pretty_assertions::assert_eq!(report.classification().status_code(), StatusCode::PAYLOAD_TOO_LARGE);
        assert!(report.detail().contains("1000000"), "{}", report.detail());
    }

    /// Clause 5.5.6 with RFC 7231 6.6.4: a store that cannot be reached is a
    /// retryable 503 with `Retry-After`, never the 500 of a fault.
    #[test]
    fn an_unreachable_store_is_a_retryable_503() {
        use andromeda_ngsi_ld_problem_details::problem_detail::ProblemClassification;
        use std::time::Duration;

        let report = SubscriptionError::StoreUnavailable.classify_for_subscription_http();
        pretty_assertions::assert_eq!(report.classification(), ProblemClassification::ServiceUnavailable);
        pretty_assertions::assert_eq!(report.retry_after(), Some(Duration::from_secs(5)));
        pretty_assertions::assert_eq!(report.detail(), "the datastore is temporarily unavailable; retry shortly");
    }
}
