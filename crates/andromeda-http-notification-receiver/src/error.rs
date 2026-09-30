//! HTTP classification of receiver failures.

use crate::state::ReceiverResolutionError;
use andromeda_http_response::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary, problem_response::ProblemResponse};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_subscription_forwarding::error::SubscriptionForwardingError;
use axum::http::Uri;

/// Classify a resolution failure.
pub(crate) fn classify_resolution(error: &ReceiverResolutionError, instance: &Uri) -> ProblemResponse {
    let report = match error {
        ReceiverResolutionError::Unknown { .. } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, error.to_string()),
        ReceiverResolutionError::ForwardingDisabled { .. } => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, error.to_string()),
        ReceiverResolutionError::Routing(_) | ReceiverResolutionError::Tenant(_) => {
            ClassifiedReport::internal(error, InternalFailureSummary::new("failed to resolve the notification tenant"))
        }
    };
    report.with_instance(instance.clone())
}

/// Classify a handling failure.
pub(crate) fn classify_forwarding(error: &SubscriptionForwardingError, instance: &Uri) -> ProblemResponse {
    let report = match error {
        SubscriptionForwardingError::UnknownSubscription { .. } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, error.to_string()),
        SubscriptionForwardingError::Representation(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
        SubscriptionForwardingError::StoreSaturated => ClassifiedReport::store_saturated(error.to_string()),
        SubscriptionForwardingError::Subscription(_)
        | SubscriptionForwardingError::Federation(_)
        | SubscriptionForwardingError::Registration(_)
        | SubscriptionForwardingError::Entity(_)
        | SubscriptionForwardingError::JsonLd(_)
        | SubscriptionForwardingError::Persistence(_)
        | SubscriptionForwardingError::Internal(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("internal notification-forwarding error")),
    };
    report.with_instance(instance.clone())
}

/// A request-shape failure the receiver reports as `BadRequestData`.
pub(crate) fn bad_request_data(detail: impl Into<String>, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::public(NgsiLdErrorType::BadRequestData, detail).with_instance(instance.clone())
}
