//! Classifying a compound request `@context` that could not be hosted.
//!
//! A response that names a compound request context in its `Link` header does
//! so through a wrapper the broker hosts (ETSI GS CIM 009 v1.9.1 Annex C.8,
//! clause 5.13.1 `ImplicitlyCreated`). When the wrapper cannot be stored the
//! response cannot name its context as clause 6.3.6 requires, so the request
//! fails; how it fails depends on why, and every route binding classifies it
//! the same way.

use crate::{classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary, store_failure::classify_store_failure};
use andromeda_jsonld::compound_context_host::ContextHostingError;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;

/// The summary disclosed for a hosting failure the client cannot act on.
const CONTEXT_HOSTING: InternalFailureSummary = InternalFailureSummary::new("the response @context could not be hosted");

/// Classify a compound request context that could not be hosted.
///
/// A saturated store, a write the store kept rolling back and a store that
/// cannot be reached are conditions the client can retry, answered 503 with
/// `Retry-After`; a request naming no
/// authority is the client's malformed request, answered 400; everything else
/// is a broker fault, logged here and answered 500.
#[must_use]
pub fn classify_context_hosting_error(error: &ContextHostingError) -> ClassifiedReport {
    match error {
        ContextHostingError::NoRequestAuthority => ClassifiedReport::public(NgsiLdErrorType::InvalidRequest, error.to_string()),
        ContextHostingError::Persistence(failure) => classify_store_failure(*failure, error, CONTEXT_HOSTING),
        ContextHostingError::NoHost | ContextHostingError::MintedUrlNotIri(_) => ClassifiedReport::internal(error, CONTEXT_HOSTING),
    }
}

#[cfg(test)]
mod tests {
    use crate::context_hosting_failure::classify_context_hosting_error;
    use andromeda_jsonld::compound_context_host::ContextHostingError;
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use std::time::Duration;

    #[test]
    fn an_unreachable_store_is_a_retryable_503() {
        let report = classify_context_hosting_error(&ContextHostingError::Persistence(PersistenceFailure::Unavailable));
        pretty_assertions::assert_eq!(report.classification(), ProblemClassification::ServiceUnavailable);
        pretty_assertions::assert_eq!(report.retry_after(), Some(Duration::from_secs(5)));
        let report = classify_context_hosting_error(&ContextHostingError::Persistence(PersistenceFailure::Misconfigured));
        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::InternalError);
        pretty_assertions::assert_eq!(report.retry_after(), None);
    }

    #[test]
    fn a_saturated_or_contended_store_is_a_retryable_503() {
        for failure in [PersistenceFailure::Saturated, PersistenceFailure::RolledBack] {
            let report = classify_context_hosting_error(&ContextHostingError::Persistence(failure));
            pretty_assertions::assert_eq!(report.classification(), ProblemClassification::ServiceUnavailable);
            assert!(report.retry_after().is_some(), "{failure:?} carries no Retry-After");
        }
    }

    /// IETF RFC 9112 section 3.2: a request naming no authority is malformed,
    /// the client's to fix.
    #[test]
    fn a_request_naming_no_authority_is_an_invalid_request() {
        let report = classify_context_hosting_error(&ContextHostingError::NoRequestAuthority);
        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::InvalidRequest);
    }

    #[test]
    fn a_missing_host_or_a_store_fault_is_an_internal_error() {
        for error in [
            ContextHostingError::NoHost,
            ContextHostingError::Persistence(PersistenceFailure::StatementRejected),
        ] {
            pretty_assertions::assert_eq!(classify_context_hosting_error(&error).error_type(), NgsiLdErrorType::InternalError);
        }
    }
}
