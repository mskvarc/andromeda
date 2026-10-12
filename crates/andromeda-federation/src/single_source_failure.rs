//! Clause 6.3.17 single-source failure classification: mapping one proxied
//! source's outcome to the typed error an operation carries when it went
//! entirely to that source.

use crate::{
    error::FederationError,
    federation_conflict_reason::FederationConflictReason,
    federation_operation_unsupported::FederationOperationUnsupported,
    gateway_failure::GatewayFailure,
    outcome::{ForwardedWriteOutcome, ForwardedWriteResult, TransportFailure},
    remote_answer::RemoteAnswer,
};
use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;

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
        ForwardedWriteOutcome::Transport(TransportFailure::TimedOut { after: timeout } | TransportFailure::CoolingDown { remaining: timeout }) => {
            Some(FederationError::GatewayTimeout {
                registration,
                timeout: *timeout,
            })
        }
        ForwardedWriteOutcome::Transport(failure @ (TransportFailure::Unprepared(_) | TransportFailure::Undelivered(_) | TransportFailure::Unreadable(_))) => {
            Some(FederationError::BadGateway {
                registration,
                cause: Box::new(GatewayFailure::Transport(failure.clone())), // clone: the error owns the failure the result keeps reporting
            })
        }
        ForwardedWriteOutcome::RemoteHttpFailure { status: 404, .. } => Some(FederationError::RemoteNotFound { registration }),
        ForwardedWriteOutcome::RemoteHttpFailure { status, body } => {
            let answer = Box::new(RemoteAnswer {
                status: *status,
                problem: body.problem(),
            });
            Some(match status {
                409 => FederationError::Conflict(FederationConflictReason::RemoteRejected(answer)),
                422 => FederationError::OperationNotSupported(FederationOperationUnsupported::RemoteUnsupported(answer)),
                _other => FederationError::BadGateway {
                    registration,
                    cause: Box::new(GatewayFailure::Answered(*answer)),
                },
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        answer_body::AnswerBody,
        error::FederationError,
        federation_invariant::FederationInvariant,
        gateway_failure::GatewayFailure,
        outcome::{ForwardedWriteOutcome, ForwardedWriteResult, TransportFailure},
        single_source_failure::single_source_failure,
    };
    use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
    use std::{sync::Arc, time::Duration};

    fn result(mode: ForwardingMode, outcome: ForwardedWriteOutcome) -> ForwardedWriteResult {
        ForwardedWriteResult {
            registration_id: "urn:ngsi-ld:csr:1".parse().expect("valid URI"),
            mode,
            outcome,
            attributes: Vec::new(),
        }
    }

    #[test]
    fn a_single_proxied_source_failure_becomes_the_clause_6_3_17_error() {
        let cases = [
            (ForwardedWriteOutcome::LoopDetected, "loop"),
            (
                ForwardedWriteOutcome::Transport(TransportFailure::TimedOut {
                    after: Duration::from_millis(5000),
                }),
                "timeout",
            ),
            (
                ForwardedWriteOutcome::Transport(TransportFailure::unprepared(
                    FederationInvariant::EndpointCannotBeBase {
                        endpoint: Box::new("data:text/plain,x".parse().expect("a valid URL")),
                    }
                    .into(),
                )),
                "unreachable",
            ),
            (
                ForwardedWriteOutcome::RemoteHttpFailure {
                    status: 404,
                    body: AnswerBody::Absent,
                },
                "not found",
            ),
            (ForwardedWriteOutcome::NotAdvertised, "not advertised"),
            (ForwardedWriteOutcome::UpsertFollowUpNotAdvertised, "no follow-up"),
            (
                ForwardedWriteOutcome::RemoteHttpFailure {
                    status: 409,
                    body: AnswerBody::Text(Arc::from(br#"{"type":"https://example.org/exists","detail":"exists"}"#.as_slice())),
                },
                "remote conflict",
            ),
            (
                ForwardedWriteOutcome::RemoteHttpFailure {
                    status: 422,
                    body: AnswerBody::Absent,
                },
                "remote unsupported",
            ),
            (
                ForwardedWriteOutcome::RemoteHttpFailure {
                    status: 403,
                    body: AnswerBody::Text(Arc::from(br#"{"type":"https://example.org/denied","detail":"no access"}"#.as_slice())),
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
                    "unreachable" => assert!(
                        matches!(&escalated, Some(FederationError::BadGateway { cause, .. }) if matches!(cause.as_ref(), GatewayFailure::Transport(_))),
                        "{mode:?} {label}"
                    ),
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
                        matches!(&escalated, Some(error @ FederationError::BadGateway { cause, .. }) if matches!(cause.as_ref(), GatewayFailure::Answered(answer) if answer.status == 403) && error.to_string().contains("no access")),
                        "{mode:?} {other}"
                    ),
                }
            }
        }
    }

    #[test]
    fn a_completed_single_source_and_an_inclusive_failure_pass_through() {
        let completed = single_source_failure(&[result(
            ForwardingMode::Exclusive,
            ForwardedWriteOutcome::Completed {
                status: 204,
                body: AnswerBody::Absent,
            },
        )]);
        assert!(completed.is_none());
        let inclusive = single_source_failure(&[result(
            ForwardingMode::Inclusive,
            ForwardedWriteOutcome::RemoteHttpFailure {
                status: 500,
                body: AnswerBody::Absent,
            },
        )]);
        assert!(inclusive.is_none());
    }

    #[test]
    fn several_sources_stay_results_for_the_207_path() {
        let results = vec![
            result(
                ForwardingMode::Redirect,
                ForwardedWriteOutcome::RemoteHttpFailure {
                    status: 500,
                    body: AnswerBody::Absent,
                },
            ),
            result(
                ForwardingMode::Redirect,
                ForwardedWriteOutcome::Completed {
                    status: 204,
                    body: AnswerBody::Absent,
                },
            ),
        ];
        assert!(single_source_failure(&results).is_none());
    }
}
