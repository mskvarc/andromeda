//! Clause 6.3.17 single-source failure classification: mapping one proxied
//! source's outcome to the typed error an operation carries when it went
//! entirely to that source.

use crate::{
    error::FederationError,
    federation_operation_unsupported::FederationOperationUnsupported,
    gateway_failure::GatewayFailure,
    outcome::{ForwardGetOutcome, ForwardedWriteOutcome, ForwardedWriteResult, TransportFailure},
    remote_answer::RemoteAnswer,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
use http::StatusCode;

/// The clause 6.3.17 error an operation carries when it went entirely to a
/// single proxied (exclusive or redirect) registration and that registration
/// failed it: 508, 504, 404 or 502, or Conflict when the registration does not
/// advertise the operation (clauses 5.6.1.4 and following). `None` when the
/// operation was spread over several sources or partly handled locally, which
/// the caller reports as a 207 instead, and when the single source succeeded.
///
/// A source refusing the write with a 409 is none of the cases clause 6.3.17
/// names, so it is a 502 carrying the source's answer, not a conflict of this
/// broker's own. A 422 stays `OperationNotSupported`, the error clause 5.6.8.4
/// gives an operation the source cannot perform.
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
        ForwardedWriteOutcome::NotAdvertised => Some(FederationError::Conflict { registration }),
        ForwardedWriteOutcome::UpsertFollowUpNotAdvertised => Some(FederationError::OperationNotSupported(
            FederationOperationUnsupported::UpsertFollowUpNotAdvertised { registration },
        )),
        // A source answering 508 found the request looping through the chain
        // this broker forwarded it along; relaying the same status keeps the
        // loop, not a bad gateway, as the operation's error.
        ForwardedWriteOutcome::LoopDetected | ForwardedWriteOutcome::RemoteHttpFailure { status: 508, .. } => {
            Some(FederationError::LoopDetected { registration })
        }
        ForwardedWriteOutcome::Transport(failure) => Some(transport_failure(registration, failure)),
        ForwardedWriteOutcome::RemoteHttpFailure { status: 404, .. } => Some(FederationError::RemoteNotFound { registration }),
        ForwardedWriteOutcome::RemoteHttpFailure { status: 422, body } => Some(FederationError::OperationNotSupported(
            FederationOperationUnsupported::RemoteUnsupported(Box::new(RemoteAnswer {
                status: 422,
                problem: body.problem(),
            })),
        )),
        ForwardedWriteOutcome::RemoteHttpFailure { status, body } => Some(FederationError::BadGateway {
            registration,
            cause: Box::new(GatewayFailure::Answered(RemoteAnswer {
                status: *status,
                problem: body.problem(),
            })),
        }),
    }
}

/// The clause 6.3.17 error of the creation of an `EntityMap` whose matching
/// registrations are the one proxied `registration`, which answered
/// `outcome`. `None` when the answer stands without failing the creation: a
/// map, a 404 (a source that declines to create a map is not abnormal, clause
/// 5.5.14), and a 501, which Table 6.34.3.1-1 relays as the operation's own
/// answer instead.
pub(crate) fn single_source_read_failure(registration: &Identifier, outcome: &ForwardGetOutcome) -> Option<FederationError> {
    let registration = registration.clone(); // clone: the error owns the identifier of the registration it describes
    match outcome {
        ForwardGetOutcome::Success(_) | ForwardGetOutcome::NotFound(_) => None,
        ForwardGetOutcome::ErrorStatus(status) if *status == StatusCode::NOT_IMPLEMENTED.as_u16() => None,
        ForwardGetOutcome::ErrorStatus(status) => Some(FederationError::BadGateway {
            registration,
            cause: Box::new(GatewayFailure::Answered(RemoteAnswer {
                status: *status,
                problem: None,
            })),
        }),
        ForwardGetOutcome::Transport(failure) => Some(transport_failure(registration, failure)),
        ForwardGetOutcome::LoopDetected => Some(FederationError::LoopDetected { registration }),
    }
}

/// The clause 6.3.17 error of a single source that gave no answer: 504 when it
/// did not answer in time or was held back by its cooldown, 502 otherwise.
fn transport_failure(registration: Identifier, failure: &TransportFailure) -> FederationError {
    match failure {
        TransportFailure::TimedOut { after: timeout } | TransportFailure::CoolingDown { remaining: timeout } => FederationError::GatewayTimeout {
            registration,
            timeout: *timeout,
        },
        TransportFailure::Unprepared(_) | TransportFailure::Undelivered(_) | TransportFailure::Refused(_) | TransportFailure::Unreadable(_) => {
            FederationError::BadGateway {
                registration,
                cause: Box::new(GatewayFailure::Transport(failure.clone())), // clone: the error owns the failure the result keeps reporting
            }
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
        outcome::{ForwardGetOutcome, ForwardedWriteOutcome, ForwardedWriteResult, TransportFailure},
        single_source_failure::{single_source_failure, single_source_read_failure},
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
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
                ForwardedWriteOutcome::RemoteHttpFailure {
                    status: 508,
                    body: AnswerBody::Absent,
                },
                "loop",
            ),
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
                    "not advertised" => assert!(matches!(escalated, Some(FederationError::Conflict { .. })), "{mode:?} {label}"),
                    "no follow-up" | "remote unsupported" => {
                        assert!(matches!(escalated, Some(FederationError::OperationNotSupported(_))), "{mode:?} {label}");
                    }
                    "remote conflict" => assert!(
                        matches!(&escalated, Some(error @ FederationError::BadGateway { cause, .. }) if matches!(cause.as_ref(), GatewayFailure::Answered(answer) if answer.status == 409) && error.to_string().contains("exists")),
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

    /// Clause 6.3.17 for the creation of an `EntityMap` over one proxied
    /// registration: a 500 is a 502, a timeout a 504 and a loop a 508. A 404 is
    /// a source declining to create a map (clause 5.5.14) and a 501 the answer
    /// Table 6.34.3.1-1 relays, so neither fails the creation here.
    #[test]
    fn a_lone_entity_map_source_failure_becomes_the_clause_6_3_17_error() {
        let registration: Identifier = "urn:ngsi-ld:csr:1".parse().expect("valid URI");
        let failed = single_source_read_failure(&registration, &ForwardGetOutcome::ErrorStatus(500));
        assert!(
            matches!(&failed, Some(FederationError::BadGateway { cause, .. }) if matches!(cause.as_ref(), GatewayFailure::Answered(answer) if answer.status == 500)),
            "{failed:?}"
        );
        let silent = single_source_read_failure(
            &registration,
            &ForwardGetOutcome::Transport(TransportFailure::TimedOut {
                after: Duration::from_millis(5000),
            }),
        );
        assert!(
            matches!(silent, Some(FederationError::GatewayTimeout { timeout, .. }) if timeout.as_millis() == 5000),
            "{silent:?}"
        );
        let looped = single_source_read_failure(&registration, &ForwardGetOutcome::LoopDetected);
        assert!(matches!(looped, Some(FederationError::LoopDetected { .. })), "{looped:?}");
        assert!(single_source_read_failure(&registration, &ForwardGetOutcome::NotFound(Vec::new())).is_none());
        assert!(single_source_read_failure(&registration, &ForwardGetOutcome::ErrorStatus(501)).is_none());
    }
}
