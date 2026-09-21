//! Answering a distributed provision (write) operation from its forwarded
//! parts and its local part.
//!
//! Clauses 5.6.1.4 and following make every write an all-or-nothing-or-partial
//! affair: the success response when every part succeeded, 207 Multi-Status
//! when some parts succeeded and some failed, and an error when the complete
//! operation failed. The current-entity and the temporal bindings answer the
//! same way, so the decision lives here, between them.

use crate::{batch_projection::forwarded_write_error, classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_federation::{error::FederationError, outcome::ForwardedWriteResult, single_source_failure::single_source_failure};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_problem_details::{
    batch_operation::{BatchEntityError, BatchOperationResult},
    error_type::NgsiLdErrorType,
    problem_detail::ProblemClassification,
};
use axum::{
    Json,
    http::{StatusCode, Uri as HttpUri},
    response::{IntoResponse, Response},
};

/// What this broker did with the part of a write that proxied registrations
/// left to it (clauses 5.6.1.4 and following: "any remaining input data").
pub enum LocalOutcome {
    /// Nothing remained once the proxied attributes were removed, so the
    /// operation went entirely to context sources.
    NothingRemained,
    /// The remainder was processed here, with this classified result.
    Processed(Result<(), ClassifiedReport>),
}

impl LocalOutcome {
    /// The outcome of a local part that ran, classified through the binding's
    /// exhaustive classifier so the report can be answered or merged into a
    /// 207 body without the binding's policy error type.
    pub fn processed<T, E>(result: Result<T, E>, classify: impl FnOnce(&E) -> ClassifiedReport) -> LocalOutcome {
        LocalOutcome::Processed(result.map(|_value| ()).map_err(|error| classify(&error)))
    }
}

/// Answer a distributed write from its forwarded results and its local part.
///
/// For an operation that went entirely to one proxied registration, a failure
/// is the clause 6.3.17 status of that failure; for one whose local part
/// failed while no source succeeded, it is the local error.
///
/// # Errors
///
/// The complete operation failed: the single proxied source's clause 6.3.17
/// error, or the local part's classified error.
pub fn provision_response(
    entity_id: &Uri,
    forwarded: &[ForwardedWriteResult],
    local: &LocalOutcome,
    success: Response,
    instance: &HttpUri,
) -> Result<Response, ProblemResponse> {
    let remote_failed = forwarded.iter().any(ForwardedWriteResult::is_failure);
    let remote_succeeded = forwarded.iter().any(|result| !result.is_failure());
    match local {
        LocalOutcome::NothingRemained => {
            if !remote_failed {
                return Ok(success);
            }
            if let Some(error) = single_source_failure(forwarded) {
                return Err(classify_forwarding_error(&error).with_instance(instance.clone()));
            }
            Ok(multi_status(multi_status_result(entity_id, local, forwarded)))
        }
        LocalOutcome::Processed(Ok(())) => {
            if remote_failed {
                Ok(multi_status(multi_status_result(entity_id, local, forwarded)))
            } else {
                Ok(success)
            }
        }
        LocalOutcome::Processed(Err(report)) => {
            if !remote_succeeded {
                return Err(report.clone().with_instance(instance.clone())); // clone: the report is owned by the outcome the caller still holds
            }
            // Table 6.6.3.1-2 reserves 207 for a partial failure. An inclusive
            // registration's local part always runs, even over an Attribute the
            // broker does not hold locally, so a local "not found" there means
            // nothing local remained to do rather than that a part failed: with
            // a source having succeeded, the operation succeeded completely
            // (204/201). A genuine local failure -- a Conflict, say -- standing
            // beside a remote success is the partial case and still answers 207.
            if report.error_type() == NgsiLdErrorType::ResourceNotFound {
                Ok(success)
            } else {
                Ok(multi_status(multi_status_result(entity_id, local, forwarded)))
            }
        }
    }
}

/// A 207 Multi-Status response carrying a clause 5.2.16 body.
///
/// Clause 6.3.6: the body is always `application/json`, whatever the
/// negotiated `Accept`, so it carries no JSON-LD context.
#[must_use]
pub fn multi_status(body: BatchOperationResult) -> Response {
    (StatusCode::MULTI_STATUS, Json(body)).into_response()
}

/// The clause 5.2.16 body of a single-entity write that partly failed: the
/// entity in `success` when any part of it landed, and one clause 5.2.17
/// error per failed part, each forwarded failure naming its `registrationId`.
#[must_use]
pub fn multi_status_result(entity_id: &Uri, local: &LocalOutcome, forwarded: &[ForwardedWriteResult]) -> BatchOperationResult {
    let mut errors = Vec::new();
    let local_ok = match local {
        LocalOutcome::NothingRemained => false,
        LocalOutcome::Processed(Ok(())) => true,
        LocalOutcome::Processed(Err(report)) => {
            errors.push(BatchEntityError::from_classification(
                entity_id.clone(),
                report.classification(),
                report.detail(),
            )); // clone: the error owns the identifier of the entity it is about
            false
        }
    };
    let mut remote_ok = false;
    for result in forwarded {
        match forwarded_write_error(entity_id.clone(), &result.registration_id, &result.outcome) {
            // clone: each error owns the identifier of the entity it is about
            Some(error) => errors.push(error),
            None => remote_ok = true,
        }
    }
    let success = if local_ok || remote_ok { vec![entity_id.clone()] } else { Vec::new() }; // clone: the body owns the identifier it reports
    BatchOperationResult { success, errors }
}

/// The clause 6.3.17 classification of a forwarding failure.
///
/// The single-source variants carry their own binding status (508, 504, 404,
/// 502) and a conflict its clause 5.5.2 category. The remaining variants
/// describe a failure inside this broker rather than at a context source, and
/// are sanitized as internal.
#[must_use]
pub fn classify_forwarding_error(error: &FederationError) -> ClassifiedReport {
    match error {
        FederationError::Conflict(reason) => ClassifiedReport::public(NgsiLdErrorType::Conflict, reason.to_string()),
        FederationError::OperationNotSupported(reason) => ClassifiedReport::public(NgsiLdErrorType::OperationNotSupported, reason.to_string()),
        FederationError::LoopDetected { .. } => ClassifiedReport::public_binding(ProblemClassification::LoopDetected, error.to_string()),
        FederationError::GatewayTimeout { .. } => ClassifiedReport::public_binding(ProblemClassification::GatewayTimeout, error.to_string()),
        FederationError::RemoteNotFound { .. } => ClassifiedReport::public_binding(ProblemClassification::DistributedResourceNotFound, error.to_string()),
        FederationError::BadGateway { .. } => ClassifiedReport::public_binding(ProblemClassification::BadGateway, error.to_string()),
        FederationError::Registration(_)
        | FederationError::JsonLd(_)
        | FederationError::Query(_)
        | FederationError::Representation(_)
        | FederationError::Internal(_) => ClassifiedReport::internal(error),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        classified_report::ClassifiedReport,
        problem_response::ProblemResponse,
        provision::{LocalOutcome, classify_forwarding_error, multi_status_result, provision_response},
    };
    use andromeda_federation::{
        error::FederationError,
        federation_conflict_reason::FederationConflictReason,
        federation_operation_unsupported::FederationOperationUnsupported,
        outcome::{ForwardedWriteOutcome, ForwardedWriteResult, TransportFailure},
    };
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
    use axum::{
        http::{StatusCode, Uri as HttpUri},
        response::{IntoResponse, Response},
    };
    use serde_json::json;
    use std::time::Duration;

    fn uri(raw: &str) -> Uri {
        raw.parse().expect("valid URI")
    }

    fn result(mode: ForwardingMode, status: u16) -> ForwardedWriteResult {
        ForwardedWriteResult {
            registration_id: uri("urn:ngsi-ld:ContextSourceRegistration:test"),
            mode,
            outcome: if status < 400 {
                ForwardedWriteOutcome::Completed { status, body: None }
            } else {
                ForwardedWriteOutcome::RemoteHttpFailure { status, body: None }
            },
            proxied_attrs: Vec::new(),
        }
    }

    fn instance() -> HttpUri {
        HttpUri::from_static("/ngsi-ld/v1/entities/urn:test:1")
    }

    fn not_found() -> ClassifiedReport {
        ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, "Entity urn:test:1 not found")
    }

    fn conflict() -> ClassifiedReport {
        ClassifiedReport::public(NgsiLdErrorType::Conflict, "Attribute urn:test:1 has multiple instances")
    }

    fn status_of(response: Result<Response, ProblemResponse>) -> u16 {
        match response {
            Ok(response) => response.status().as_u16(),
            Err(problem) => problem.into_response().status().as_u16(),
        }
    }

    #[test]
    fn an_entirely_remote_write_answered_by_one_failing_proxied_source_is_the_clause_6_3_17_error() {
        for (remote, expected) in [(404, 404), (403, 502), (500, 502)] {
            let forwarded = vec![result(ForwardingMode::Exclusive, remote)];
            let response = provision_response(
                &uri("urn:test:1"),
                &forwarded,
                &LocalOutcome::NothingRemained,
                StatusCode::NO_CONTENT.into_response(),
                &instance(),
            );
            pretty_assertions::assert_eq!(status_of(response), expected, "remote {remote}");
        }
    }

    #[test]
    fn a_single_proxied_source_that_does_not_advertise_the_operation_is_a_conflict() {
        let mut single = result(ForwardingMode::Redirect, 409);
        single.outcome = ForwardedWriteOutcome::NotAdvertised;
        let response = provision_response(
            &uri("urn:test:1"),
            &[single],
            &LocalOutcome::NothingRemained,
            StatusCode::NO_CONTENT.into_response(),
            &instance(),
        );
        pretty_assertions::assert_eq!(status_of(response), 409);
    }

    #[test]
    fn several_sources_with_one_failure_and_nothing_local_are_a_207() {
        let forwarded = vec![result(ForwardingMode::Redirect, 204), result(ForwardingMode::Redirect, 500)];
        let response = provision_response(
            &uri("urn:test:1"),
            &forwarded,
            &LocalOutcome::NothingRemained,
            StatusCode::NO_CONTENT.into_response(),
            &instance(),
        );
        pretty_assertions::assert_eq!(status_of(response), 207);
    }

    #[test]
    fn a_local_success_beside_a_remote_failure_is_a_partial_success() {
        let forwarded = vec![result(ForwardingMode::Exclusive, 500)];
        let response = provision_response(
            &uri("urn:test:1"),
            &forwarded,
            &LocalOutcome::Processed(Ok(())),
            StatusCode::CREATED.into_response(),
            &instance(),
        );
        pretty_assertions::assert_eq!(status_of(response), 207);
    }

    /// Clause 5.6.6.4: a proxied registration that does not advertise Delete
    /// Entity cannot delete its part, but a local row that was actually removed is
    /// a succeeded part, so the operation is a 207 partial success rather than the
    /// 409 it is when nothing was removed locally (the `NothingRemained` case above).
    #[test]
    fn an_unsupported_proxied_delete_beside_a_successful_local_removal_is_a_partial_success() {
        let mut single = result(ForwardingMode::Exclusive, 409);
        single.outcome = ForwardedWriteOutcome::NotAdvertised;
        let response = provision_response(
            &uri("urn:test:1"),
            &[single],
            &LocalOutcome::Processed(Ok(())),
            StatusCode::NO_CONTENT.into_response(),
            &instance(),
        );
        pretty_assertions::assert_eq!(status_of(response), 207);
    }

    #[test]
    fn every_part_succeeding_answers_with_the_success_response() {
        let forwarded = vec![result(ForwardingMode::Inclusive, 204)];
        let created = provision_response(
            &uri("urn:test:1"),
            &forwarded,
            &LocalOutcome::Processed(Ok(())),
            StatusCode::CREATED.into_response(),
            &instance(),
        );
        pretty_assertions::assert_eq!(status_of(created), 201);
        let remote_only = provision_response(
            &uri("urn:test:1"),
            &forwarded,
            &LocalOutcome::NothingRemained,
            StatusCode::NO_CONTENT.into_response(),
            &instance(),
        );
        pretty_assertions::assert_eq!(status_of(remote_only), 204);
    }

    /// Table 6.6.3.1-2: an inclusive registration's local part always runs, so
    /// a local `ResourceNotFound` over an Attribute the broker does not hold is
    /// "nothing remained locally", not a failed part. With a source having
    /// succeeded, the operation succeeded completely and answers 204, not 207.
    #[test]
    fn a_local_not_found_beside_a_remote_success_is_a_complete_success() {
        let response = provision_response(
            &uri("urn:test:1"),
            &[result(ForwardingMode::Inclusive, 204)],
            &LocalOutcome::Processed(Err(not_found())),
            StatusCode::NO_CONTENT.into_response(),
            &instance(),
        );
        pretty_assertions::assert_eq!(status_of(response), 204);
    }

    /// The exclusion is only for "not found": a genuine local failure -- a
    /// Conflict -- standing beside a remote success is the partial case Table
    /// 6.6.3.1-2 answers with 207.
    #[test]
    fn a_genuine_local_failure_beside_a_remote_success_is_a_partial_success() {
        let response = provision_response(
            &uri("urn:test:1"),
            &[result(ForwardingMode::Inclusive, 204)],
            &LocalOutcome::Processed(Err(conflict())),
            StatusCode::NO_CONTENT.into_response(),
            &instance(),
        );
        pretty_assertions::assert_eq!(status_of(response), 207);
    }

    #[test]
    fn a_local_failure_is_the_error_when_no_source_succeeded() {
        let complete_failure = provision_response(
            &uri("urn:test:1"),
            &[result(ForwardingMode::Inclusive, 500)],
            &LocalOutcome::Processed(Err(not_found())),
            StatusCode::NO_CONTENT.into_response(),
            &instance(),
        );
        pretty_assertions::assert_eq!(status_of(complete_failure), 404);
    }

    #[test]
    fn the_multi_status_body_names_the_registration_of_every_forwarded_failure() {
        // Table 5.2.17-1: `registrationId` is the registration responsible for a
        // failed distributed operation; the local failure names none.
        let mut timed_out = result(ForwardingMode::Inclusive, 504);
        timed_out.outcome = ForwardedWriteOutcome::Transport(TransportFailure::Timeout {
            detail: "no response within 5000ms".to_string(),
        });
        timed_out.registration_id = uri("urn:ngsi-ld:ContextSourceRegistration:slow");
        let body = serde_json::to_value(multi_status_result(
            &uri("urn:test:1"),
            &LocalOutcome::Processed(Err(not_found())),
            &[result(ForwardingMode::Inclusive, 500), timed_out],
        ))
        .expect("serialises");
        let errors = body["errors"].as_array().expect("errors array");
        pretty_assertions::assert_eq!(errors.len(), 3);
        assert!(errors[0].get("registrationId").is_none(), "the local failure names no registration: {body}");
        pretty_assertions::assert_eq!(errors[1]["registrationId"], json!("urn:ngsi-ld:ContextSourceRegistration:test"));
        pretty_assertions::assert_eq!(errors[1]["error"]["status"], json!(500));
        pretty_assertions::assert_eq!(errors[2]["registrationId"], json!("urn:ngsi-ld:ContextSourceRegistration:slow"));
        pretty_assertions::assert_eq!(errors[2]["error"]["status"], json!(504));
        assert!(body["success"].as_array().is_some_and(Vec::is_empty), "nothing landed: {body}");
    }

    #[test]
    fn the_multi_status_body_lists_the_entity_once_when_any_part_landed() {
        let local_ok = multi_status_result(&uri("urn:test:1"), &LocalOutcome::Processed(Ok(())), &[result(ForwardingMode::Inclusive, 500)]);
        pretty_assertions::assert_eq!(local_ok.success, vec![uri("urn:test:1")]);
        pretty_assertions::assert_eq!(local_ok.errors.len(), 1);

        let remote_ok = multi_status_result(
            &uri("urn:test:1"),
            &LocalOutcome::Processed(Err(conflict())),
            &[result(ForwardingMode::Inclusive, 204), result(ForwardingMode::Inclusive, 204)],
        );
        pretty_assertions::assert_eq!(remote_ok.success, vec![uri("urn:test:1")]);
        pretty_assertions::assert_eq!(remote_ok.errors.len(), 1);
    }

    #[test]
    fn forwarding_errors_classify_to_their_clause_6_3_17_binding() {
        let registration = uri("urn:ngsi-ld:ContextSourceRegistration:test");
        for (error, expected) in [
            (
                FederationError::Conflict(FederationConflictReason::RemoteRejected {
                    detail: "not advertised".to_string(),
                }),
                ProblemClassification::Category(NgsiLdErrorType::Conflict),
            ),
            (
                FederationError::OperationNotSupported(FederationOperationUnsupported::RemoteUnsupported {
                    detail: "no follow-up".to_string(),
                }),
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
                    registration,
                    status: Some(403),
                    detail: "denied".to_string(),
                },
                ProblemClassification::BadGateway,
            ),
            (
                FederationError::internal("broken"),
                ProblemClassification::Category(NgsiLdErrorType::InternalError),
            ),
        ] {
            pretty_assertions::assert_eq!(classify_forwarding_error(&error).classification(), expected, "{error}");
        }
    }
}
