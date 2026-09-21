//! Clause 6.3.17 single-source failure classification: mapping one proxied
//! source's outcome to the typed error an operation carries when it went
//! entirely to that source.

use crate::{
    error::FederationError,
    federation_conflict_reason::FederationConflictReason,
    federation_operation_unsupported::FederationOperationUnsupported,
    outcome::{ForwardedWriteOutcome, ForwardedWriteResult, TransportFailure},
};
use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
use serde_json::Value;
use std::time::Duration;

/// Clause 6.3.17: when the data lives in a single exclusive or redirect
/// registration, its failure is the operation's failure and carries the code
/// the clause assigns; when several endpoints were contacted, the results are
/// returned for the 207 Multi-Status path.
/// The clause 6.3.17 error an operation carries when it went entirely to a
/// single proxied (exclusive or redirect) registration and that registration
/// failed it: 508, 504, 404 or 502, or Conflict when the registration does not
/// advertise the operation (clauses 5.6.1.4 and following). `None` when the
/// operation was spread over several sources or partly handled locally, which
/// the caller reports as a 207 instead, and when the single source succeeded.
#[must_use]
pub fn single_source_failure(results: &[ForwardedWriteResult]) -> Option<FederationError> {
    let [single] = results else {
        return None;
    };
    if !matches!(single.mode, ForwardingMode::Exclusive | ForwardingMode::Redirect) {
        return None;
    }
    let registration = single.registration_id.clone(); // clone: the error owns the identifier of the result it describes
    match &single.outcome {
        ForwardedWriteOutcome::Completed { .. } => None,
        ForwardedWriteOutcome::NotAdvertised => Some(FederationError::Conflict(FederationConflictReason::NotAdvertised { registration })),
        ForwardedWriteOutcome::UpsertFollowUpNotAdvertised => Some(FederationError::OperationNotSupported(
            FederationOperationUnsupported::UpsertFollowUpNotAdvertised { registration },
        )),
        ForwardedWriteOutcome::LoopDetected => Some(FederationError::LoopDetected { registration }),
        ForwardedWriteOutcome::Transport(TransportFailure::Timeout { detail }) => Some(FederationError::GatewayTimeout {
            registration,
            timeout: parse_timeout_detail(detail),
        }),
        ForwardedWriteOutcome::Transport(TransportFailure::Unreachable { detail }) => Some(FederationError::BadGateway {
            registration,
            status: None,
            detail: detail.clone(), // clone: same
        }),
        ForwardedWriteOutcome::RemoteHttpFailure { status: 404, .. } => Some(FederationError::RemoteNotFound { registration }),
        // Clause 6.3.17 asks for the most informative error response; a
        // source that refused the operation itself (409, 422) is reported as
        // that refusal rather than folded into 502 Bad Gateway.
        ForwardedWriteOutcome::RemoteHttpFailure { status: status @ 409, body } => Some(FederationError::Conflict(FederationConflictReason::RemoteRejected {
            detail: remote_detail(*status, body.as_ref()),
        })),
        ForwardedWriteOutcome::RemoteHttpFailure { status: status @ 422, body } => {
            Some(FederationError::OperationNotSupported(FederationOperationUnsupported::RemoteUnsupported {
                detail: remote_detail(*status, body.as_ref()),
            }))
        }
        ForwardedWriteOutcome::RemoteHttpFailure { status, body } => Some(FederationError::BadGateway {
            registration,
            status: Some(*status),
            detail: remote_detail(*status, body.as_ref()),
        }),
    }
}

/// The most informative detail a remote error carries: its problem `detail`,
/// its `title`, or the bare status.
fn remote_detail(status: u16, body: Option<&Value>) -> String {
    body.and_then(|body| body.get("detail").or_else(|| body.get("title")))
        .and_then(Value::as_str)
        .map_or_else(
            || format!("context source answered HTTP {status}"),
            |detail| format!("context source answered HTTP {status}: {detail}"),
        )
}

/// Recover the deadline a timeout detail names, for the typed error.
fn parse_timeout_detail(detail: &str) -> Duration {
    detail
        .split(|character: char| !character.is_ascii_digit())
        .filter(|token| !token.is_empty())
        .find_map(|token| token.parse::<u64>().ok())
        .map_or(Duration::ZERO, Duration::from_millis)
}

#[cfg(test)]
mod tests {
    use crate::{
        error::FederationError,
        outcome::{ForwardedWriteOutcome, ForwardedWriteResult, TransportFailure},
        single_source_failure::{remote_detail, single_source_failure},
    };
    use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
    use serde_json::json;

    fn result(mode: ForwardingMode, outcome: ForwardedWriteOutcome) -> ForwardedWriteResult {
        ForwardedWriteResult {
            registration_id: "urn:ngsi-ld:csr:1".parse().expect("valid URI"),
            mode,
            outcome,
            proxied_attrs: Vec::new(),
        }
    }

    #[test]
    fn a_single_proxied_source_failure_becomes_the_clause_6_3_17_error() {
        let cases = [
            (ForwardedWriteOutcome::LoopDetected, "loop"),
            (
                ForwardedWriteOutcome::Transport(TransportFailure::Timeout {
                    detail: "no response within 5000ms".to_string(),
                }),
                "timeout",
            ),
            (
                ForwardedWriteOutcome::Transport(TransportFailure::Unreachable {
                    detail: "connection refused".to_string(),
                }),
                "unreachable",
            ),
            (ForwardedWriteOutcome::RemoteHttpFailure { status: 404, body: None }, "not found"),
            (ForwardedWriteOutcome::NotAdvertised, "not advertised"),
            (ForwardedWriteOutcome::UpsertFollowUpNotAdvertised, "no follow-up"),
            (
                ForwardedWriteOutcome::RemoteHttpFailure {
                    status: 409,
                    body: Some(json!({"detail": "exists"})),
                },
                "remote conflict",
            ),
            (ForwardedWriteOutcome::RemoteHttpFailure { status: 422, body: None }, "remote unsupported"),
            (
                ForwardedWriteOutcome::RemoteHttpFailure {
                    status: 403,
                    body: Some(json!({"detail": "no access"})),
                },
                "forbidden",
            ),
        ];
        for mode in [ForwardingMode::Exclusive, ForwardingMode::Redirect] {
            for (outcome, label) in cases.clone() {
                let escalated = single_source_failure(&[result(mode, outcome)]);
                match label {
                    "loop" => assert!(matches!(escalated, Some(FederationError::LoopDetected { .. })), "{mode:?} {label}"),
                    "timeout" => assert!(
                        matches!(escalated, Some(FederationError::GatewayTimeout { timeout, .. }) if timeout.as_millis() == 5000),
                        "{mode:?} {label}"
                    ),
                    "unreachable" => assert!(matches!(escalated, Some(FederationError::BadGateway { status: None, .. })), "{mode:?} {label}"),
                    "not found" => assert!(matches!(escalated, Some(FederationError::RemoteNotFound { .. })), "{mode:?} {label}"),
                    "not advertised" => assert!(matches!(escalated, Some(FederationError::Conflict(_))), "{mode:?} {label}"),
                    "no follow-up" | "remote unsupported" => {
                        assert!(matches!(escalated, Some(FederationError::OperationNotSupported(_))), "{mode:?} {label}");
                    }
                    "remote conflict" => assert!(
                        matches!(&escalated, Some(error @ FederationError::Conflict(_)) if error.to_string().contains("exists")),
                        "{mode:?} {label}"
                    ),
                    other => assert!(
                        matches!(escalated, Some(FederationError::BadGateway { status: Some(403), ref detail, .. }) if detail.contains("no access")),
                        "{mode:?} {other}"
                    ),
                }
            }
        }
    }

    #[test]
    fn a_completed_single_source_and_an_inclusive_failure_pass_through() {
        let completed = single_source_failure(&[result(ForwardingMode::Exclusive, ForwardedWriteOutcome::Completed { status: 204, body: None })]);
        assert!(completed.is_none());
        let inclusive = single_source_failure(&[result(
            ForwardingMode::Inclusive,
            ForwardedWriteOutcome::RemoteHttpFailure { status: 500, body: None },
        )]);
        assert!(inclusive.is_none());
    }

    #[test]
    fn several_sources_stay_results_for_the_207_path() {
        let results = vec![
            result(ForwardingMode::Redirect, ForwardedWriteOutcome::RemoteHttpFailure { status: 500, body: None }),
            result(ForwardingMode::Redirect, ForwardedWriteOutcome::Completed { status: 204, body: None }),
        ];
        assert!(single_source_failure(&results).is_none());
    }

    #[test]
    fn the_remote_detail_prefers_the_problem_detail_then_the_title_then_the_status() {
        pretty_assertions::assert_eq!(
            remote_detail(403, Some(&json!({"detail": "denied"}))),
            "context source answered HTTP 403: denied"
        );
        pretty_assertions::assert_eq!(
            remote_detail(403, Some(&json!({"title": "Forbidden"}))),
            "context source answered HTTP 403: Forbidden"
        );
        pretty_assertions::assert_eq!(remote_detail(500, None), "context source answered HTTP 500");
    }
}
