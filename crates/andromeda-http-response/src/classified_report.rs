//! Client-safe classification and disclosure before response construction.

use crate::{internal_failure_summary::InternalFailureSummary, problem_response::ProblemResponse};
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

/// The `Retry-After` delay attached to a 503 for a transaction the datastore
/// kept rolling back.
///
/// The broker has already re-run the transaction within its own budget, so the
/// conflict outlived several attempts; a one-second hint separates the retry
/// from the contenders it collided with.
const TRANSACTION_CONTENDED_RETRY_AFTER: Duration = Duration::from_secs(1);

/// The `Retry-After` delay attached to a 503 for a datastore that cannot be
/// reached.
///
/// A restart, failover or pooler reconnect takes seconds, and a pooler caches a
/// failed server login for its retry window; a one-second hint would multiply
/// the load on a store that is coming back.
const STORE_UNAVAILABLE_RETRY_AFTER: Duration = Duration::from_secs(5);

/// A classified, client-safe failure without request-boundary state.
///
/// Route bindings construct public reports from deliberate detail or use [`ClassifiedReport::internal`] to log a typed source exactly once and replace its disclosure with a coarse, client-safe operation summary.
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
    /// disclosed rather than masked to a coarse operation summary.
    pub fn store_saturated(detail: impl Into<String>) -> ClassifiedReport {
        ClassifiedReport::public_binding(ProblemClassification::ServiceUnavailable, detail).with_retry_after(STORE_SATURATED_RETRY_AFTER)
    }

    /// Report a transaction the datastore rolled back on every attempt as a 503
    /// Service Unavailable with a `Retry-After`.
    ///
    /// For a write that lost a deadlock or serialization conflict to concurrent
    /// transactions each time the broker ran it. Nothing of it persisted and the
    /// request is well-formed, so it can be sent again unchanged: answered 503,
    /// never the 500 an internal fault earns, with its detail disclosed.
    pub fn transaction_contended(detail: impl Into<String>) -> ClassifiedReport {
        ClassifiedReport::public_binding(ProblemClassification::ServiceUnavailable, detail).with_retry_after(TRANSACTION_CONTENDED_RETRY_AFTER)
    }

    /// Report a datastore that could not be reached as a 503 Service
    /// Unavailable with a `Retry-After`.
    ///
    /// For a request that met a store which was restarting, failing over or
    /// cut off, or whose connection was lost under it. Nothing of the request
    /// is known to have persisted, and every write the broker offers is
    /// idempotent in the store or answers a repeat with `AlreadyExists`, so it
    /// can be sent again once the store is back: answered 503 with its fixed
    /// detail disclosed.
    pub fn store_unavailable(detail: impl Into<String>) -> ClassifiedReport {
        ClassifiedReport::public_binding(ProblemClassification::ServiceUnavailable, detail).with_retry_after(STORE_UNAVAILABLE_RETRY_AFTER)
    }

    /// Mask an internal failure with a vetted operation summary after logging its typed source at the disclosure boundary.
    ///
    /// The real cause is logged once here, inside the surrounding `http-request`
    /// span whose `x-request-id` the client already holds, so an operator can
    /// correlate the coarse summary back to the fully-detailed line. `summary` is
    /// a compile-time literal (see [`InternalFailureSummary`]) and never carries
    /// any part of the cause.
    pub fn internal(source: &impl Display, summary: InternalFailureSummary) -> ClassifiedReport {
        tracing::error!(error = %source, "Internal error sanitized before sending to the client");
        ClassifiedReport {
            classification: ProblemClassification::Category(NgsiLdErrorType::InternalError),
            detail: summary.to_string(),
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::classified_report::ClassifiedReport;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        thread::yield_now,
    };
    use tracing::{
        Event,
        Level,
        Subscriber,
        callsite::rebuild_interest_cache,
        metadata::Metadata,
        span::{Attributes, Id, Record},
        subscriber::with_default,
    };

    /// How many times the callsite interest cache is rebuilt before an error
    /// that never arrives fails the test.
    const INTEREST_REBUILD_ATTEMPTS: usize = 1_000;

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
    fn store_unavailability_is_a_503_with_a_five_second_retry_after() {
        use andromeda_ngsi_ld_problem_details::problem_detail::ProblemClassification;
        use axum::http::StatusCode;
        use std::time::Duration;

        let report = ClassifiedReport::store_unavailable("the datastore is temporarily unavailable; retry shortly");
        pretty_assertions::assert_eq!(report.classification(), ProblemClassification::ServiceUnavailable);
        pretty_assertions::assert_eq!(report.classification().status_code(), StatusCode::SERVICE_UNAVAILABLE);
        pretty_assertions::assert_eq!(
            report.classification().type_iri().as_str(),
            "https://uri.etsi.org/ngsi-ld/errors/ServiceUnavailable"
        );
        pretty_assertions::assert_eq!(report.retry_after(), Some(Duration::from_secs(5)));
        pretty_assertions::assert_eq!(report.detail(), "the datastore is temporarily unavailable; retry shortly");
    }

    #[test]
    fn internal_discloses_its_summary_but_never_the_raw_cause() {
        use crate::internal_failure_summary::InternalFailureSummary;

        let errors = Arc::new(AtomicUsize::new(0));
        let subscriber = CountingSubscriber { errors: Arc::clone(&errors) };
        let report = with_default(subscriber, || {
            // Other tests log through the same callsite on threads with no
            // subscriber, and a callsite first registered there caches that
            // no one listens. A registration still in flight on such a thread
            // stores that answer after any rebuild that ran before it
            // finished, so the cache is rebuilt until the error reaches this
            // subscriber.
            let mut report = None;
            for _ in 0..INTEREST_REBUILD_ATTEMPTS {
                rebuild_interest_cache();
                report = Some(ClassifiedReport::internal(
                    &"database db-7.internal failed",
                    InternalFailureSummary::ENTITY_STORE_FAULT,
                ));
                if errors.load(Ordering::SeqCst) > 0 {
                    break;
                }
                yield_now();
            }
            report
        })
        .expect("at least one report was built");
        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::InternalError);
        pretty_assertions::assert_eq!(report.detail(), "the entity store could not complete the operation");
        assert!(!report.detail().contains("db-7.internal"));
        pretty_assertions::assert_eq!(errors.load(Ordering::SeqCst), 1);
    }
}
