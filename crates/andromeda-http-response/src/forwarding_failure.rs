//! Classifying a failure of the distributed part of an operation.
//!
//! Clause 6.3.17 gives a forwarded request's failures their own binding
//! status. Reading the registrations to forward to is the first statement of
//! every distributed operation, so a store outage surfaces here first; it is
//! the broker's own store failing and is answered as one (clause 5.5.6, 503
//! with `Retry-After` for a condition that clears), never as a context source
//! that could not be reached.

use crate::{
    classified_report::ClassifiedReport,
    internal_failure_summary::InternalFailureSummary,
    jsonld_failure::classify_jsonld_error,
    query_failure::classify_query_error,
};
use andromeda_federation::error::FederationError;
use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
use andromeda_registration::error::RegistrationError;

/// The clause 6.3.17 classification of a forwarding failure.
///
/// The single-source variants carry their own binding status (508, 504, 404,
/// 502) and a conflict its clause 5.5.2 category. A failure to read the
/// registrations to forward to is this broker's own store failing, never a
/// context source, and is classified by its store condition
/// ([`registration_store_failure`]). The remaining variants describe a failure
/// inside this broker while it handled a context source's answer, and are
/// sanitized as internal; a write payload that could not be serialised for
/// forwarding is the broker's own encoding failure.
#[must_use]
pub fn classify_forwarding_error(error: &FederationError) -> ClassifiedReport {
    match error {
        FederationError::Conflict { .. } => ClassifiedReport::public(NgsiLdErrorType::Conflict, error.to_string()),
        FederationError::OperationNotSupported(reason) => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, reason.to_string()),
        FederationError::LoopDetected { .. } => ClassifiedReport::public_binding(ProblemClassification::LoopDetected, error.to_string()),
        FederationError::GatewayTimeout { .. } => ClassifiedReport::public_binding(ProblemClassification::GatewayTimeout, error.to_string()),
        FederationError::RemoteNotFound { .. } => ClassifiedReport::public_binding(ProblemClassification::DistributedResourceNotFound, error.to_string()),
        FederationError::BadGateway { .. } => ClassifiedReport::public_binding(ProblemClassification::BadGateway, error.to_string()),
        // A `q` the reached sources cannot read as asked, or an identifier list
        // no split carries within this broker's forwarded URL budget, is
        // request data the operation cannot meet (clause 5.5.2).
        FederationError::TargetLanguageUnadvertised { .. } | FederationError::ForwardedUrlOverBudget { .. } => {
            ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string())
        }
        FederationError::Registration(source) => registration_store_failure(source),
        FederationError::JsonLd(_)
        | FederationError::Query(_)
        | FederationError::Compaction(_)
        | FederationError::Representation(_)
        | FederationError::Internal(_) => ClassifiedReport::internal(error, InternalFailureSummary::CONTEXT_FEDERATION),
        FederationError::ForwardedBody(_) => ClassifiedReport::internal(error, InternalFailureSummary::RESPONSE_ENCODING),
    }
}

/// The classification a route binding gives a failure of the distributed part
/// of its operation.
///
/// A JSON-LD or query failure is classified as the same failure of the
/// client's own request would be; every other failure as clause 6.3.17
/// classifies a forwarding failure ([`classify_forwarding_error`]).
#[must_use]
pub fn classify_federation_error(error: &FederationError) -> ClassifiedReport {
    match error {
        FederationError::JsonLd(source) => classify_jsonld_error(source),
        FederationError::Query(source) => classify_query_error(source),
        FederationError::Conflict { .. }
        | FederationError::OperationNotSupported(_)
        | FederationError::LoopDetected { .. }
        | FederationError::GatewayTimeout { .. }
        | FederationError::RemoteNotFound { .. }
        | FederationError::BadGateway { .. }
        | FederationError::TargetLanguageUnadvertised { .. }
        | FederationError::ForwardedUrlOverBudget { .. }
        | FederationError::Registration(_)
        | FederationError::Compaction(_)
        | FederationError::Representation(_)
        | FederationError::ForwardedBody(_)
        | FederationError::Internal(_) => classify_forwarding_error(error),
    }
}

/// The classification of a failure to read the registrations to forward to.
///
/// A saturated or unreachable registration store is a retryable 503, exactly
/// as the same condition of any other store; a store fault is an internal
/// error naming the registration store. A lookup raises none of the request
/// failures the type also carries, so they are classified as the lookup
/// failing.
#[must_use]
pub fn registration_store_failure(error: &RegistrationError) -> ClassifiedReport {
    match error {
        RegistrationError::StoreSaturated => ClassifiedReport::store_saturated(error.to_string()),
        RegistrationError::StoreUnavailable => ClassifiedReport::store_unavailable(error.to_string()),
        RegistrationError::Persistence(_) | RegistrationError::Internal(_) => {
            ClassifiedReport::internal(error, InternalFailureSummary::REGISTRATION_STORE_FAULT)
        }
        RegistrationError::Validation(_)
        | RegistrationError::NotFound { .. }
        | RegistrationError::AlreadyExists { .. }
        | RegistrationError::Conflict(_)
        | RegistrationError::JsonLd(_)
        | RegistrationError::GeometryTooLarge { .. }
        | RegistrationError::Query(_) => ClassifiedReport::internal(error, InternalFailureSummary::new("failed to select the registrations to forward to")),
    }
}

/// The answer a read owes when its distributed part failed because this
/// broker's own registration store is saturated or unreachable.
///
/// A read whose forwarding fails at a context source carries on with what the
/// broker holds and a clause 6.3.17 warning. A read that could not even learn
/// which sources exist cannot: answering local data alone would hide every
/// source without a warning. Only that local store condition is returned; every
/// other failure is `None`, left to the read's own handling.
#[must_use]
pub fn local_federation_failure(error: &FederationError) -> Option<ClassifiedReport> {
    match error {
        FederationError::Registration(store @ (RegistrationError::StoreSaturated | RegistrationError::StoreUnavailable)) => {
            Some(registration_store_failure(store))
        }
        FederationError::Registration(
            RegistrationError::Validation(_)
            | RegistrationError::NotFound { .. }
            | RegistrationError::AlreadyExists { .. }
            | RegistrationError::Conflict(_)
            | RegistrationError::JsonLd(_)
            | RegistrationError::GeometryTooLarge { .. }
            | RegistrationError::Query(_)
            | RegistrationError::Persistence(_)
            | RegistrationError::Internal(_),
        )
        | FederationError::Conflict { .. }
        | FederationError::OperationNotSupported(_)
        | FederationError::LoopDetected { .. }
        | FederationError::GatewayTimeout { .. }
        | FederationError::RemoteNotFound { .. }
        | FederationError::BadGateway { .. }
        | FederationError::TargetLanguageUnadvertised { .. }
        | FederationError::ForwardedUrlOverBudget { .. }
        | FederationError::JsonLd(_)
        | FederationError::Query(_)
        | FederationError::Compaction(_)
        | FederationError::Representation(_)
        | FederationError::ForwardedBody(_)
        | FederationError::Internal(_) => None,
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        classified_report::ClassifiedReport,
        forwarding_failure::{classify_forwarding_error, local_federation_failure},
        internal_failure_summary::InternalFailureSummary,
    };
    use andromeda_federation::{
        error::FederationError,
        federation_invariant::FederationInvariant,
        federation_operation_unsupported::FederationOperationUnsupported,
        gateway_failure::GatewayFailure,
        remote_answer::RemoteAnswer,
    };
    use andromeda_http_headers::received_protocol::ReceivedProtocol;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_registration::error::RegistrationError;
    use serde_json::Value;
    use std::time::Duration;

    fn uri(raw: &str) -> Identifier {
        raw.parse::<Identifier>().expect("a valid identifier")
    }

    /// Clause 5.5.6 with RFC 7231 6.6.4: the registration lookup failing on a
    /// saturated or unreachable store is the broker's own store, answered 503
    /// with `Retry-After`; a store fault is a 500 naming the registration
    /// store. None of them is reported as a context source failure.
    #[test]
    fn a_registration_store_condition_is_not_a_context_source_failure() {
        for (failure, expected, retry_after, detail) in [
            (
                RegistrationError::StoreSaturated,
                ProblemClassification::ServiceUnavailable,
                Some(Duration::from_secs(1)),
                "the datastore connection pool is saturated; retry shortly",
            ),
            (
                RegistrationError::StoreUnavailable,
                ProblemClassification::ServiceUnavailable,
                Some(Duration::from_secs(5)),
                "the datastore is temporarily unavailable; retry shortly",
            ),
            (
                RegistrationError::Persistence(PersistenceFailure::StatementRejected),
                ProblemClassification::Category(NgsiLdErrorType::InternalError),
                None,
                "the registration store could not complete the operation",
            ),
        ] {
            let report = classify_forwarding_error(&FederationError::Registration(failure));
            pretty_assertions::assert_eq!(report.classification(), expected, "{detail}");
            pretty_assertions::assert_eq!(report.retry_after(), retry_after, "{detail}");
            pretty_assertions::assert_eq!(report.detail(), detail);
            assert_ne!(report.detail(), InternalFailureSummary::CONTEXT_FEDERATION.to_string());
        }
    }

    /// A read carries on past a context source that failed, but not past its
    /// own registration store being saturated or unreachable.
    #[test]
    fn only_a_local_store_condition_interrupts_a_federated_read() {
        for failure in [RegistrationError::StoreSaturated, RegistrationError::StoreUnavailable] {
            let report = local_federation_failure(&FederationError::Registration(failure));
            pretty_assertions::assert_eq!(report.map(|report| report.classification()), Some(ProblemClassification::ServiceUnavailable));
        }
        let registration = uri("urn:ngsi-ld:ContextSourceRegistration:test");
        for error in [
            FederationError::GatewayTimeout {
                registration: registration.clone(), // clone: each case owns its registration
                timeout: Duration::from_secs(5),
            },
            FederationError::BadGateway {
                registration,
                cause: Box::new(GatewayFailure::Answered(RemoteAnswer { status: 500, problem: None })),
            },
            FederationError::Registration(RegistrationError::Persistence(PersistenceFailure::StatementRejected)),
        ] {
            assert!(local_federation_failure(&error).is_none(), "{error}");
        }
    }

    #[test]
    fn forwarding_errors_classify_to_their_clause_6_3_17_binding() {
        let registration = uri("urn:ngsi-ld:ContextSourceRegistration:test");
        for (error, expected) in [
            (
                FederationError::Conflict {
                    registration: registration.clone(), // clone: each case owns its registration
                },
                ProblemClassification::Category(NgsiLdErrorType::Conflict),
            ),
            (
                FederationError::OperationNotSupported(FederationOperationUnsupported::RemoteUnsupported(Box::new(RemoteAnswer {
                    status: 422,
                    problem: None,
                }))),
                ProblemClassification::Category(NgsiLdErrorType::OperationNotSupported),
            ),
            (
                FederationError::LoopDetected {
                    registration: registration.clone(), // clone: each case owns its registration
                },
                ProblemClassification::LoopDetected,
            ),
            (
                FederationError::GatewayTimeout {
                    registration: registration.clone(), // clone: same
                    timeout: Duration::from_secs(5),
                },
                ProblemClassification::GatewayTimeout,
            ),
            (
                FederationError::RemoteNotFound {
                    registration: registration.clone(), // clone: same
                },
                ProblemClassification::DistributedResourceNotFound,
            ),
            (
                FederationError::BadGateway {
                    registration: registration.clone(), // clone: same
                    cause: Box::new(GatewayFailure::Answered(RemoteAnswer { status: 403, problem: None })),
                },
                ProblemClassification::BadGateway,
            ),
            (
                FederationError::ForwardedUrlOverBudget {
                    registration,
                    budget: 8192,
                    smallest: 9000,
                },
                ProblemClassification::Category(NgsiLdErrorType::BadRequestData),
            ),
            (
                FederationError::from(FederationInvariant::ForwardingProtocol(
                    "".parse::<ReceivedProtocol>().expect_err("an empty protocol is not a Via protocol"),
                )),
                ProblemClassification::Category(NgsiLdErrorType::InternalError),
            ),
        ] {
            pretty_assertions::assert_eq!(classify_forwarding_error(&error).classification(), expected, "{error}");
        }
    }

    /// A write payload serialised only once a registration matched fails, when
    /// it cannot be serialised, the way the handler's own serialisation of it
    /// failed before it was deferred: as the broker's encoding failure.
    #[test]
    fn a_forwarded_payload_that_cannot_be_serialised_is_the_encoding_failure_it_always_was() {
        let unserialisable = || serde_json::from_str::<Value>("{").expect_err("an unterminated object is malformed");
        let forwarded = classify_forwarding_error(&FederationError::ForwardedBody(unserialisable()));
        let encoded = ClassifiedReport::internal(&unserialisable(), InternalFailureSummary::RESPONSE_ENCODING);
        pretty_assertions::assert_eq!(forwarded.classification(), encoded.classification());
        pretty_assertions::assert_eq!(forwarded.detail(), encoded.detail());
    }
}
