//! Client-safe classification and disclosure before response construction.

use crate::problem_response::ProblemResponse;
use andromeda_ngsi_ld_problem_details::{
    error_type::NgsiLdErrorType,
    problem_detail::{ProblemClassification, ProblemDetail},
};
use axum::http::Uri;
use std::fmt::Display;

/// A classified, client-safe failure without request-boundary state.
///
/// Route bindings construct public reports from deliberate detail or use [`ClassifiedReport::internal`] to log a typed source exactly once and replace its disclosure with `Internal error`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassifiedReport {
    /// Typed category and binding-specific status.
    classification: ProblemClassification,
    /// Client-safe detail.
    detail: String,
}

impl ClassifiedReport {
    /// Construct a public ordinary-category report with deliberate client-visible detail.
    pub fn public(error_type: NgsiLdErrorType, detail: impl Into<String>) -> ClassifiedReport {
        ClassifiedReport {
            classification: ProblemClassification::Category(error_type),
            detail: detail.into(),
        }
    }

    /// Construct a public report with a binding-specific typed status.
    pub fn public_binding(classification: ProblemClassification, detail: impl Into<String>) -> ClassifiedReport {
        ClassifiedReport {
            classification,
            detail: detail.into(),
        }
    }

    /// Sanitize an internal failure after logging its typed source at the disclosure boundary.
    pub fn internal(source: &impl Display) -> ClassifiedReport {
        tracing::error!(error = %source, "Internal error sanitized before sending to the client");
        ClassifiedReport {
            classification: ProblemClassification::Category(NgsiLdErrorType::InternalError),
            detail: "Internal error".to_string(),
        }
    }

    /// The typed classification selected by the route binding.
    pub const fn classification(&self) -> ProblemClassification {
        self.classification
    }

    /// The clause 5.5.2 payload category.
    pub const fn error_type(&self) -> NgsiLdErrorType {
        self.classification.error_type()
    }

    /// The client-safe human-readable detail.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Attach the inbound request URI and create the local Axum response error.
    pub fn with_instance(self, instance: Uri) -> ProblemResponse {
        ProblemResponse::new(ProblemDetail::from_classification(self.classification, self.detail).with_instance(instance))
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
    fn internal_detail_is_always_sanitized() {
        let errors = Arc::new(AtomicUsize::new(0));
        let subscriber = CountingSubscriber { errors: Arc::clone(&errors) };
        let report = tracing::subscriber::with_default(subscriber, || ClassifiedReport::internal(&"database db-7.internal failed"));
        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::InternalError);
        pretty_assertions::assert_eq!(report.detail(), "Internal error");
        assert!(!report.detail().contains("db-7.internal"));
        pretty_assertions::assert_eq!(errors.load(Ordering::SeqCst), 1);
    }
}
