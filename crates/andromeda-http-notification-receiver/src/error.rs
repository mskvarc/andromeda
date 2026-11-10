//! HTTP classification of receiver failures.

use crate::state::ReceiverResolutionError;
use andromeda_failure_classification::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_subscription_forwarding::{error::SubscriptionForwardingError, inbound::InboundError};
use axum::http::Uri;
use thiserror::Error;

/// Classify a resolution failure.
pub(crate) fn classify_resolution(error: &ReceiverResolutionError, instance: &Uri) -> ProblemResponse {
    let report = match error {
        ReceiverResolutionError::Unknown { .. } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, error.to_string()),
        ReceiverResolutionError::ForwardingDisabled { .. } => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, error.to_string()),
        ReceiverResolutionError::Routing(_) | ReceiverResolutionError::Tenant(_) => {
            ClassifiedReport::internal(error, InternalFailureSummary::new("failed to resolve the notification tenant"))
        }
    };
    ProblemResponse::classified(report, instance.clone())
}

/// Classify a handling failure.
pub(crate) fn classify_forwarding(error: &SubscriptionForwardingError, instance: &Uri) -> ProblemResponse {
    let report = match error {
        SubscriptionForwardingError::UnknownSubscription { .. } => ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, error.to_string()),
        SubscriptionForwardingError::Representation(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
        SubscriptionForwardingError::StoreSaturated => ClassifiedReport::store_saturated(error.to_string()),
        SubscriptionForwardingError::StoreUnavailable => ClassifiedReport::store_unavailable(error.to_string()),
        SubscriptionForwardingError::Subscription(_)
        | SubscriptionForwardingError::Federation(_)
        | SubscriptionForwardingError::Registration(_)
        | SubscriptionForwardingError::Entity(_)
        | SubscriptionForwardingError::JsonLd(_)
        | SubscriptionForwardingError::Persistence(_)
        | SubscriptionForwardingError::Internal(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("internal notification-forwarding error")),
    };
    ProblemResponse::classified(report, instance.clone())
}

/// A request body that is neither of the two notification documents the receiver accepts.
#[derive(Debug, Error)]
#[error("not a notification: {0}")]
struct NotANotification(#[source] InboundError);

/// Classify a body that is not a notification as `BadRequestData`.
pub(crate) fn classify_not_a_notification(error: InboundError, instance: &Uri) -> ProblemResponse {
    ProblemResponse::classified(
        ClassifiedReport::public(NgsiLdErrorType::BadRequestData, NotANotification(error).to_string()),
        instance.clone(),
    )
}

#[cfg(test)]
mod tests {
    use crate::error::classify_forwarding;
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_subscription_forwarding::error::SubscriptionForwardingError;
    use axum::http::Uri;

    fn instance() -> Uri {
        Uri::from_static("/ngsi-ld/v1/csourceNotify")
    }

    /// Clause 5.5.6 with RFC 7231 6.6.4: a saturated or unreachable store is a
    /// retryable 503; a store fault stays an internal 500.
    #[test]
    fn an_unreachable_store_is_a_retryable_503() {
        for error in [SubscriptionForwardingError::StoreSaturated, SubscriptionForwardingError::StoreUnavailable] {
            let response = classify_forwarding(&error, &instance());
            pretty_assertions::assert_eq!(response.problem().classification(), ProblemClassification::ServiceUnavailable, "{error}");
        }
        let fault = classify_forwarding(&SubscriptionForwardingError::Persistence(PersistenceFailure::StatementRejected), &instance());
        pretty_assertions::assert_eq!(
            fault.problem().classification(),
            ProblemClassification::Category(NgsiLdErrorType::InternalError)
        );
    }
}
