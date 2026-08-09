//! Forwarding mode classification and multi-status response building for
//! distributed provision (write) operations.
//!
//! This module encapsulates the common forwarding result classification logic
//! that was duplicated across 8 entity write handlers. Each handler calls
//! `forward_provision`, then uses these helpers to classify the result modes
//! and build multi-status responses.

use crate::entity_error::classify_entity_error;
use andromeda_entity::error::EntityError;
use andromeda_federation::outcome::ForwardedWriteResult;
use andromeda_http_response::batch_projection::batch_entity_error;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::registration::ForwardingMode;
use serde_json::Value;

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
                    "detail": r.transport_detail()
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

/// Check whether any remote forwarding result has an error status (>= 400).
pub fn has_remote_errors(forwarded: &[ForwardedWriteResult]) -> bool {
    forwarded.iter().any(ForwardedWriteResult::is_failure)
}

/// Get the HTTP status code from the first forwarded result, with a fallback.
///
/// Used by redirect and exclusive modes to propagate the context source's
/// response status.
pub fn first_forwarded_status(forwarded: &[ForwardedWriteResult], fallback: u16) -> u16 {
    forwarded.first().map_or(fallback, ForwardedWriteResult::status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_federation::outcome::ForwardedWriteOutcome;

    fn make_result(mode: ForwardingMode, status: u16) -> ForwardedWriteResult {
        ForwardedWriteResult {
            mode,
            outcome: if status < 400 {
                ForwardedWriteOutcome::Completed { status, body: None }
            } else {
                ForwardedWriteOutcome::RemoteHttpFailure { status, body: None }
            },
            exclusive_attrs: Vec::new(),
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

    #[test]
    fn has_remote_errors_none() {
        let results = vec![make_result(ForwardingMode::Inclusive, 204)];
        assert!(!has_remote_errors(&results));
    }

    #[test]
    fn has_remote_errors_present() {
        let results = vec![make_result(ForwardingMode::Inclusive, 204), make_result(ForwardingMode::Inclusive, 500)];
        assert!(has_remote_errors(&results));
    }

    #[test]
    fn first_forwarded_status_with_results() {
        let results = vec![make_result(ForwardingMode::Redirect, 301)];
        pretty_assertions::assert_eq!(first_forwarded_status(&results, 204), 301);
    }

    #[test]
    fn first_forwarded_status_empty_uses_fallback() {
        pretty_assertions::assert_eq!(first_forwarded_status(&[], 204), 204);
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
