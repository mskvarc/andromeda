//! Forwarding mode classification and multi-status response building for
//! distributed provision (write) operations.
//!
//! This module encapsulates the common forwarding result classification logic
//! that was duplicated across 8 entity write handlers. Each handler calls
//! `forward_provision`, then uses these helpers to classify the result modes
//! and build multi-status responses.

use crate::{entity_error::classify_entity_error, error::EntityResultExt};
use andromeda_entity::error::EntityError;
use andromeda_federation::{outcome::ForwardedWriteResult, provision::single_source_failure};
use andromeda_http_response::{batch_projection::batch_entity_error, problem_response::ProblemResponse};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::registration::ForwardingMode;
use axum::{
    Json,
    http::{StatusCode, Uri as HttpUri},
    response::{IntoResponse, Response},
};
use serde_json::Value;

/// What this broker did with the part of a write that proxied registrations
/// left to it (clauses 5.6.1.4 and following: "any remaining input data").
pub enum LocalOutcome {
    /// Nothing remained once the proxied attributes were removed, so the
    /// operation went entirely to context sources.
    NothingRemained,
    /// The remainder was processed here, with this result.
    Processed(Result<(), EntityError>),
}

/// Answer a distributed write from its forwarded results and its local part.
///
/// Clauses 5.6.1.4 and following make every write an all-or-nothing-or-partial
/// affair: `success` when every part succeeded, 207 when some parts succeeded
/// and some failed, and an error when the complete operation failed. For an
/// operation that went entirely to one proxied registration, that error is the
/// clause 6.3.17 status of the failure; for one whose local part failed, it is
/// the local error.
pub fn provision_response(
    entity_id: &Uri,
    forwarded: &[ForwardedWriteResult],
    local: LocalOutcome,
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
                return Err::<Response, _>(error).with_entity_instance(instance);
            }
            Ok(multi_status(build_multi_status_body(entity_id, false, None, forwarded)))
        }
        LocalOutcome::Processed(Ok(())) => {
            if remote_failed {
                Ok(multi_status(build_multi_status_body(entity_id, true, None, forwarded)))
            } else {
                Ok(success)
            }
        }
        LocalOutcome::Processed(Err(error)) => {
            if remote_succeeded {
                Ok(multi_status(build_multi_status_body(entity_id, false, Some(&error), forwarded)))
            } else {
                Err(error).with_entity_instance(instance)
            }
        }
    }
}

/// A 207 Multi-Status response carrying `body`.
fn multi_status(body: Value) -> Response {
    (StatusCode::MULTI_STATUS, Json(body)).into_response()
}

/// Classified forwarding modes present in a set of forwarded write results.
///
/// Computed once from the forwarded results and used to determine the local
/// operation and response strategy.
pub struct ForwardingModes {
    /// At least one CSR matched with exclusive mode.
    pub has_exclusive: bool,
    /// At least one CSR matched with redirect mode.
    pub has_redirect: bool,
    /// At least one CSR matched with inclusive mode.
    pub has_inclusive: bool,
}

impl ForwardingModes {
    /// Classify the forwarding modes from a set of forwarded write results.
    pub fn from_results(forwarded: &[ForwardedWriteResult]) -> ForwardingModes {
        ForwardingModes {
            has_exclusive: forwarded.iter().any(|r| r.mode == ForwardingMode::Exclusive),
            has_redirect: forwarded.iter().any(|r| r.mode == ForwardingMode::Redirect),
            has_inclusive: forwarded.iter().any(|r| r.mode == ForwardingMode::Inclusive),
        }
    }
}

/// Build the success/errors lists for a 207 Multi-Status response from
/// inclusive forwarding with local operation results.
///
/// Used by create, delete, append_attrs, update_attrs, and delete_attr
/// handlers when inclusive mode produces mixed local+remote results.
pub fn build_multi_status_body(entity_id: &Uri, local_ok: bool, local_error: Option<&EntityError>, forwarded: &[ForwardedWriteResult]) -> Value {
    let mut success = Vec::new();
    let mut errors = Vec::new();

    if local_ok {
        success.push(serde_json::json!(entity_id));
    } else if let Some(err) = local_error {
        let projected = batch_entity_error(entity_id.clone(), err, classify_entity_error);
        errors.push(serde_json::json!({
            "entityId": entity_id,
            "error": projected.error
        }));
    }

    for r in forwarded {
        if r.is_failure() {
            let error_body = r.body().cloned().unwrap_or_else(|| {
                // clone: owned Value needed for JSON response body
                serde_json::json!({
                    "status": r.status(),
                    "title": "Error from context source",
                    "detail": r.failure_detail()
                })
            });
            errors.push(serde_json::json!({
                "entityId": entity_id,
                "error": error_body
            }));
        } else if !local_ok {
            success.push(serde_json::json!(entity_id));
        }
    }

    serde_json::json!({
        "success": success,
        "errors": errors
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_federation::outcome::ForwardedWriteOutcome;

    fn make_result(mode: ForwardingMode, status: u16) -> ForwardedWriteResult {
        ForwardedWriteResult {
            registration_id: entity_uri("urn:ngsi-ld:ContextSourceRegistration:test"),
            mode,
            outcome: if status < 400 {
                ForwardedWriteOutcome::Completed { status, body: None }
            } else {
                ForwardedWriteOutcome::RemoteHttpFailure { status, body: None }
            },
            proxied_attrs: Vec::new(),
        }
    }

    fn entity_uri(raw: &str) -> Uri {
        raw.parse().expect("valid entity URI")
    }

    #[test]
    fn forwarding_modes_empty_results() {
        let modes = ForwardingModes::from_results(&[]);
        assert!(!modes.has_exclusive);
        assert!(!modes.has_redirect);
        assert!(!modes.has_inclusive);
    }

    #[test]
    fn forwarding_modes_detects_exclusive() {
        let results = vec![make_result(ForwardingMode::Exclusive, 204)];
        let modes = ForwardingModes::from_results(&results);
        assert!(modes.has_exclusive);
        assert!(!modes.has_redirect);
        assert!(!modes.has_inclusive);
    }

    #[test]
    fn forwarding_modes_detects_redirect() {
        let results = vec![make_result(ForwardingMode::Redirect, 204)];
        let modes = ForwardingModes::from_results(&results);
        assert!(!modes.has_exclusive);
        assert!(modes.has_redirect);
        assert!(!modes.has_inclusive);
    }

    #[test]
    fn forwarding_modes_detects_inclusive() {
        let results = vec![make_result(ForwardingMode::Inclusive, 204)];
        let modes = ForwardingModes::from_results(&results);
        assert!(!modes.has_exclusive);
        assert!(!modes.has_redirect);
        assert!(modes.has_inclusive);
    }

    #[test]
    fn forwarding_modes_mixed() {
        let results = vec![make_result(ForwardingMode::Exclusive, 204), make_result(ForwardingMode::Inclusive, 200)];
        let modes = ForwardingModes::from_results(&results);
        assert!(modes.has_exclusive);
        assert!(!modes.has_redirect);
        assert!(modes.has_inclusive);
    }

    fn instance() -> HttpUri {
        HttpUri::from_static("/ngsi-ld/v1/entities/urn:test:1")
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
            let forwarded = vec![make_result(ForwardingMode::Exclusive, remote)];
            let response = provision_response(
                &entity_uri("urn:test:1"),
                &forwarded,
                LocalOutcome::NothingRemained,
                StatusCode::NO_CONTENT.into_response(),
                &instance(),
            );
            pretty_assertions::assert_eq!(status_of(response), expected, "remote {remote}");
        }
    }

    #[test]
    fn a_single_proxied_source_that_does_not_advertise_the_operation_is_a_conflict() {
        let mut result = make_result(ForwardingMode::Redirect, 409);
        result.outcome = ForwardedWriteOutcome::NotAdvertised;
        let response = provision_response(
            &entity_uri("urn:test:1"),
            &[result],
            LocalOutcome::NothingRemained,
            StatusCode::NO_CONTENT.into_response(),
            &instance(),
        );
        pretty_assertions::assert_eq!(status_of(response), 409);
    }

    #[test]
    fn several_sources_with_one_failure_and_nothing_local_are_a_207() {
        let forwarded = vec![make_result(ForwardingMode::Redirect, 204), make_result(ForwardingMode::Redirect, 500)];
        let response = provision_response(
            &entity_uri("urn:test:1"),
            &forwarded,
            LocalOutcome::NothingRemained,
            StatusCode::NO_CONTENT.into_response(),
            &instance(),
        );
        pretty_assertions::assert_eq!(status_of(response), 207);
    }

    #[test]
    fn a_local_success_beside_a_remote_failure_is_a_partial_success() {
        let forwarded = vec![make_result(ForwardingMode::Exclusive, 500)];
        let response = provision_response(
            &entity_uri("urn:test:1"),
            &forwarded,
            LocalOutcome::Processed(Ok(())),
            StatusCode::CREATED.into_response(),
            &instance(),
        );
        pretty_assertions::assert_eq!(status_of(response), 207);
    }

    #[test]
    fn every_part_succeeding_answers_with_the_success_response() {
        let forwarded = vec![make_result(ForwardingMode::Inclusive, 204)];
        let response = provision_response(
            &entity_uri("urn:test:1"),
            &forwarded,
            LocalOutcome::Processed(Ok(())),
            StatusCode::CREATED.into_response(),
            &instance(),
        );
        pretty_assertions::assert_eq!(status_of(response), 201);
        let response = provision_response(
            &entity_uri("urn:test:1"),
            &forwarded,
            LocalOutcome::NothingRemained,
            StatusCode::NO_CONTENT.into_response(),
            &instance(),
        );
        pretty_assertions::assert_eq!(status_of(response), 204);
    }

    #[test]
    fn a_local_failure_is_the_error_only_when_no_source_succeeded() {
        let id = entity_uri("urn:test:1");
        let partial = provision_response(
            &id,
            &[make_result(ForwardingMode::Inclusive, 204)],
            LocalOutcome::Processed(Err(EntityError::not_found(&id))),
            StatusCode::NO_CONTENT.into_response(),
            &instance(),
        );
        pretty_assertions::assert_eq!(status_of(partial), 207);
        let complete_failure = provision_response(
            &id,
            &[make_result(ForwardingMode::Inclusive, 500)],
            LocalOutcome::Processed(Err(EntityError::not_found(&id))),
            StatusCode::NO_CONTENT.into_response(),
            &instance(),
        );
        pretty_assertions::assert_eq!(status_of(complete_failure), 404);
    }

    #[test]
    fn build_multi_status_body_local_success_remote_error() {
        let remote = vec![make_result(ForwardingMode::Inclusive, 500)];
        let body = build_multi_status_body(&entity_uri("urn:test:1"), true, None, &remote);
        let success = body["success"].as_array().expect("success array");
        let errors = body["errors"].as_array().expect("errors array");
        pretty_assertions::assert_eq!(success.len(), 1);
        pretty_assertions::assert_eq!(errors.len(), 1);
    }

    #[test]
    fn build_multi_status_body_local_error_remote_success() {
        let id = entity_uri("urn:test:1");
        let err = EntityError::not_found(&id);
        let remote = vec![make_result(ForwardingMode::Inclusive, 204)];
        let body = build_multi_status_body(&entity_uri("urn:test:1"), false, Some(&err), &remote);
        let success = body["success"].as_array().expect("success array");
        let errors = body["errors"].as_array().expect("errors array");
        pretty_assertions::assert_eq!(success.len(), 1);
        pretty_assertions::assert_eq!(errors.len(), 1);
    }

    #[test]
    fn build_multi_status_body_all_success() {
        let remote = vec![make_result(ForwardingMode::Inclusive, 204)];
        let body = build_multi_status_body(&entity_uri("urn:test:1"), true, None, &remote);
        let success = body["success"].as_array().expect("success array");
        let errors = body["errors"].as_array().expect("errors array");
        pretty_assertions::assert_eq!(success.len(), 1);
        assert!(errors.is_empty());
    }
}
