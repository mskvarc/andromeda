//! Client-safe classification and disclosure before response construction.

use crate::problem_response::ProblemResponse;
use andromeda_ngsi_ld_problem_details::{
    error_type::NgsiLdErrorType,
    problem_detail::{ProblemClassification, ProblemDetail},
};
use axum::http::Uri;
use std::{fmt::Display, time::Duration};

/// The `Retry-After` delay attached to a shed-backpressure 503.
///
/// A saturated pool clears on the order of one request, so a one-second hint is
/// long enough to matter and short enough not to strand a client.
const STORE_SATURATED_RETRY_AFTER: Duration = Duration::from_secs(1);

/// A classified, client-safe failure without request-boundary state.
///
/// Route bindings construct public reports from deliberate detail or use [`ClassifiedReport::internal`] to log a typed source exactly once and replace its disclosure with `internal error`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassifiedReport {
    /// Typed category and binding-specific status.
    classification: ProblemClassification,
    /// Client-safe detail.
    detail: String,
    /// Optional retry delay selected by a recoverable binding policy.
    retry_after: Option<Duration>,
}

impl ClassifiedReport {
    /// Construct a public ordinary-category report with deliberate client-visible detail.
    pub fn public(error_type: NgsiLdErrorType, detail: impl Into<String>) -> ClassifiedReport {
        ClassifiedReport {
            classification: ProblemClassification::Category(error_type),
            detail: detail.into(),
            retry_after: None,
        }
    }

    /// Construct a public report with a binding-specific typed status.
    pub fn public_binding(classification: ProblemClassification, detail: impl Into<String>) -> ClassifiedReport {
        ClassifiedReport {
            classification,
            detail: detail.into(),
            retry_after: None,
        }
    }

    /// Shed backpressure as a 503 Service Unavailable with a `Retry-After`.
    ///
    /// For a request the broker declined to run because a resource was
    /// momentarily at capacity — a saturated connection pool is the canonical
    /// case. The request is well-formed and can be retried unchanged, so it is
    /// answered 503, never the 500 an internal fault earns, and its detail is
    /// disclosed rather than sanitized to `internal error`. Clause 6.3.2 defines
    /// no `ProblemDetails` type for 503, so the existing `ServiceUnavailable`
    /// classification handling applies.
    pub fn store_saturated(detail: impl Into<String>) -> ClassifiedReport {
        ClassifiedReport::public_binding(ProblemClassification::ServiceUnavailable, detail).with_retry_after(STORE_SATURATED_RETRY_AFTER)
    }

    /// Sanitize an internal failure after logging its typed source at the disclosure boundary.
    pub fn internal(source: &impl Display) -> ClassifiedReport {
        tracing::error!(error = %source, "Internal error sanitized before sending to the client");
        ClassifiedReport {
            classification: ProblemClassification::Category(NgsiLdErrorType::InternalError),
            detail: "internal error".to_string(),
            retry_after: None,
        }
    }

    /// The typed classification selected by the route binding.
    #[must_use]
    pub const fn classification(&self) -> ProblemClassification {
        self.classification
    }

    /// The clause 5.5.2 payload category.
    #[must_use]
    pub const fn error_type(&self) -> NgsiLdErrorType {
        self.classification.error_type()
    }

    /// The client-safe human-readable detail.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Attach a bounded retry delay to a recoverable response.
    #[must_use]
    pub const fn with_retry_after(mut self, retry_after: Duration) -> ClassifiedReport {
        self.retry_after = Some(retry_after);
        self
    }

    /// The bounded retry delay a recoverable binding attached, if any.
    #[must_use]
    pub const fn retry_after(&self) -> Option<Duration> {
        self.retry_after
    }

    /// Attach the inbound request URI and create the local Axum response error.
    pub fn with_instance(self, instance: Uri) -> ProblemResponse {
        let response = ProblemResponse::new(ProblemDetail::from_classification(self.classification, self.detail).with_instance(instance));
        match self.retry_after {
            Some(retry_after) => response.with_retry_after(retry_after),
            None => response,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::classified_report::ClassifiedReport;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tracing::{
        Event,
        Level,
        Subscriber,
        metadata::Metadata,
        span::{Attributes, Id, Record},
        subscriber::with_default,
    };

    struct CountingSubscriber {
        errors: Arc<AtomicUsize>,
    }

    impl Subscriber for CountingSubscriber {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, event: &Event<'_>) {
            if *event.metadata().level() == Level::ERROR {
                self.errors.fetch_add(1, Ordering::SeqCst);
            }
        }

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}
    }

    #[test]
    fn public_detail_is_preserved() {
        let report = ClassifiedReport::public(NgsiLdErrorType::BadRequestData, "missing id");
        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::BadRequestData);
        pretty_assertions::assert_eq!(report.detail(), "missing id");
    }

    #[test]
    fn store_saturation_is_a_503_with_retry_after() {
        use andromeda_ngsi_ld_problem_details::problem_detail::ProblemClassification;
        use axum::http::StatusCode;
        use std::time::Duration;

        let report = ClassifiedReport::store_saturated("the datastore connection pool is saturated");
        pretty_assertions::assert_eq!(report.classification(), ProblemClassification::ServiceUnavailable);
        pretty_assertions::assert_eq!(report.classification().status_code(), StatusCode::SERVICE_UNAVAILABLE);
        pretty_assertions::assert_eq!(report.retry_after(), Some(Duration::from_secs(1)));
        // The detail is disclosed, not sanitized: it names backpressure, not a fault.
        assert!(report.detail().contains("saturated"), "{}", report.detail());
    }

    #[test]
    fn internal_detail_is_always_sanitized() {
        let errors = Arc::new(AtomicUsize::new(0));
        let subscriber = CountingSubscriber { errors: Arc::clone(&errors) };
        let report = with_default(subscriber, || ClassifiedReport::internal(&"database db-7.internal failed"));
        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::InternalError);
        pretty_assertions::assert_eq!(report.detail(), "internal error");
        assert!(!report.detail().contains("db-7.internal"));
        pretty_assertions::assert_eq!(errors.load(Ordering::SeqCst), 1);
    }
}
