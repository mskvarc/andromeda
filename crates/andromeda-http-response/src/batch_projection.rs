//! Projection of policy batch outcomes and forwarded write outcomes into
//! clause 5.2.16 wire values.

use crate::{classified_report::ClassifiedReport, forwarded_write_failure::ForwardedWriteFailure};
use andromeda_federation::{outcome::ForwardedWriteOutcome, remote_answer::RemoteAnswer};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, uri::Uri};
use andromeda_ngsi_ld_problem_details::{
    batch_operation::{BatchEntityError, BatchErrorKind, BatchOperationResult, BatchProblemDetail},
    error_type::NgsiLdErrorType,
    problem_detail::ProblemClassification,
};

/// Project one policy error through its binding-supplied exhaustive classifier.
pub fn batch_entity_error<E>(entity_id: Uri, error: &E, classify: impl FnOnce(&E) -> ClassifiedReport) -> BatchEntityError {
    let report = classify(error);
    BatchEntityError::new(entity_id, report.error_type(), report.detail())
}

/// Project a complete policy batch outcome into the clause 5.2.16 wire envelope.
pub fn batch_operation_result<E>(success: Vec<Identifier>, errors: Vec<(Identifier, E)>, classify: impl Fn(&E) -> ClassifiedReport) -> BatchOperationResult {
    let errors = errors
        .into_iter()
        .map(|(entity_id, error)| batch_entity_error(entity_id.into(), &error, &classify))
        .collect();
    BatchOperationResult { success, errors }
}

/// The clause 5.2.17 error a forwarded write contributes for `entity_id`, or
/// `None` when the context source completed it.
///
/// The error is the [`forwarded_write_problem`], and names the registration
/// it came from (Table 5.2.17-1 `registrationId`).
#[must_use]
pub fn forwarded_write_error(entity_id: Uri, registration_id: &Identifier, outcome: &ForwardedWriteOutcome) -> Option<BatchEntityError> {
    let error = forwarded_write_problem(registration_id, outcome)?;
    Some(BatchEntityError {
        entity_id,
        registration_id: Some(registration_id.clone()), // clone: the error owns the identifier of the registration it names
        error,
    })
}

/// The problem detail a forwarded write failed with, or `None` when the
/// context source completed it.
///
/// A problem detail the source answered with is carried through as it came
/// (RFC 7807 permits any type); any other failure is classified from what
/// happened, with the clause 6.3.17 statuses for the ones that produced no
/// response.
#[must_use]
pub fn forwarded_write_problem(registration_id: &Identifier, outcome: &ForwardedWriteOutcome) -> Option<BatchProblemDetail> {
    let (classification, failure) = match outcome {
        ForwardedWriteOutcome::Completed { .. } => return None,
        ForwardedWriteOutcome::RemoteHttpFailure { status, body } => match body.problem() {
            Some(problem) => return Some(problem),
            None => (
                classification_for_remote_status(*status),
                ForwardedWriteFailure::RemoteStatus(RemoteAnswer {
                    status: *status,
                    problem: None,
                }),
            ),
        },
        ForwardedWriteOutcome::Transport(failure) => {
            let classification = if failure.is_timeout() {
                ProblemClassification::GatewayTimeout
            } else {
                ProblemClassification::BadGateway
            };
            (classification, ForwardedWriteFailure::Transport(failure))
        }
        ForwardedWriteOutcome::LoopDetected => (
            ProblemClassification::LoopDetected,
            ForwardedWriteFailure::LoopDetected { registration: registration_id },
        ),
        ForwardedWriteOutcome::NotAdvertised => (
            ProblemClassification::Category(NgsiLdErrorType::Conflict),
            ForwardedWriteFailure::NotAdvertised { registration: registration_id },
        ),
        ForwardedWriteOutcome::UpsertFollowUpNotAdvertised => (
            ProblemClassification::Category(NgsiLdErrorType::OperationNotSupported),
            ForwardedWriteFailure::UpsertFollowUpNotAdvertised { registration: registration_id },
        ),
    };
    Some(BatchProblemDetail {
        error_type: BatchErrorKind::Known(classification),
        detail: failure.to_string(),
    })
}

/// The classification of a source's error status when its body carried no
/// problem detail to read one from: the clause 6.3.2 categories for the
/// statuses they own, the clause 6.3.17 distributed statuses for 504 and 508,
/// and 502 Bad Gateway for anything else the source answered with.
const fn classification_for_remote_status(status: u16) -> ProblemClassification {
    match status {
        400 => ProblemClassification::Category(NgsiLdErrorType::BadRequestData),
        404 => ProblemClassification::Category(NgsiLdErrorType::ResourceNotFound),
        409 => ProblemClassification::Category(NgsiLdErrorType::Conflict),
        422 => ProblemClassification::Category(NgsiLdErrorType::OperationNotSupported),
        500 => ProblemClassification::Category(NgsiLdErrorType::InternalError),
        504 => ProblemClassification::GatewayTimeout,
        508 => ProblemClassification::LoopDetected,
        _other => ProblemClassification::BadGateway,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        batch_projection::{batch_operation_result, forwarded_write_error},
        classified_report::ClassifiedReport,
    };
    use andromeda_federation::{
        answer_body::AnswerBody,
        federation_invariant::FederationInvariant,
        outcome::{ForwardedWriteOutcome, TransportFailure},
    };
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, uri::Uri};
    use andromeda_ngsi_ld_problem_details::{batch_operation::BatchErrorKind, error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
    use serde_json::json;
    use std::{error::Error, sync::Arc, time::Duration};

    fn text(body: &serde_json::Value) -> AnswerBody {
        AnswerBody::Text(Arc::from(body.to_string().into_bytes()))
    }

    fn uri(raw: &str) -> Identifier {
        raw.parse().expect("valid URI")
    }

    fn entity(raw: &str) -> Uri {
        raw.parse().expect("valid URI")
    }

    #[test]
    fn a_completed_forward_contributes_no_error() {
        assert!(
            forwarded_write_error(
                entity("urn:ngsi-ld:Entity:1"),
                &uri("urn:ngsi-ld:ContextSourceRegistration:1"),
                &ForwardedWriteOutcome::Completed {
                    status: 204,
                    body: AnswerBody::Absent
                }
            )
            .is_none()
        );
    }

    #[test]
    fn a_sources_problem_detail_is_carried_through_under_its_registration() {
        let error = forwarded_write_error(
            entity("urn:ngsi-ld:Entity:1"),
            &uri("urn:ngsi-ld:ContextSourceRegistration:1"),
            &ForwardedWriteOutcome::RemoteHttpFailure {
                status: 409,
                body: text(&json!({"type": "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists", "title": "exists", "status": 409})),
            },
        )
        .expect("a failure contributes an error");
        pretty_assertions::assert_eq!(error.registration_id, Some(uri("urn:ngsi-ld:ContextSourceRegistration:1")));
        pretty_assertions::assert_eq!(
            error.error.error_type,
            BatchErrorKind::Known(ProblemClassification::Category(NgsiLdErrorType::AlreadyExists))
        );
        pretty_assertions::assert_eq!(error.error.detail, "exists");
    }

    #[test]
    fn a_failure_without_a_problem_detail_is_classified_from_its_status() {
        for (status, expected) in [
            (404, ProblemClassification::Category(NgsiLdErrorType::ResourceNotFound)),
            (409, ProblemClassification::Category(NgsiLdErrorType::Conflict)),
            (422, ProblemClassification::Category(NgsiLdErrorType::OperationNotSupported)),
            (403, ProblemClassification::BadGateway),
            (504, ProblemClassification::GatewayTimeout),
        ] {
            let error = forwarded_write_error(
                entity("urn:ngsi-ld:Entity:1"),
                &uri("urn:ngsi-ld:ContextSourceRegistration:1"),
                &ForwardedWriteOutcome::RemoteHttpFailure {
                    status,
                    body: text(&json!({"message": "not a problem detail"})),
                },
            )
            .expect("a failure contributes an error");
            pretty_assertions::assert_eq!(error.error.error_type, BatchErrorKind::Known(expected), "{status}");
            assert!(error.error.detail.contains(&status.to_string()), "{}", error.error.detail);
        }
    }

    #[test]
    fn the_failures_that_produced_no_response_carry_the_clause_6_3_17_classification() {
        let registration = uri("urn:ngsi-ld:ContextSourceRegistration:1");
        for (outcome, expected) in [
            (
                ForwardedWriteOutcome::Transport(TransportFailure::TimedOut {
                    after: Duration::from_millis(5000),
                }),
                ProblemClassification::GatewayTimeout,
            ),
            (
                ForwardedWriteOutcome::Transport(TransportFailure::unprepared(
                    FederationInvariant::EndpointCannotBeBase {
                        endpoint: Box::new("data:text/plain,x".parse().expect("a valid URL")),
                    }
                    .into(),
                )),
                ProblemClassification::BadGateway,
            ),
            (ForwardedWriteOutcome::LoopDetected, ProblemClassification::LoopDetected),
            (ForwardedWriteOutcome::NotAdvertised, ProblemClassification::Category(NgsiLdErrorType::Conflict)),
            (
                ForwardedWriteOutcome::UpsertFollowUpNotAdvertised,
                ProblemClassification::Category(NgsiLdErrorType::OperationNotSupported),
            ),
        ] {
            let error = forwarded_write_error(entity("urn:ngsi-ld:Entity:1"), &registration, &outcome).expect("a failure contributes an error");
            pretty_assertions::assert_eq!(error.error.error_type, BatchErrorKind::Known(expected), "{outcome:?}");
            pretty_assertions::assert_eq!(error.registration_id.as_ref(), Some(&registration));
        }
    }

    #[test]
    fn projection_preserves_successes_failures_and_supplied_classification() -> Result<(), Box<dyn Error>> {
        let success = "urn:ngsi-ld:Entity:1".parse::<Identifier>()?;
        let failed = "urn:ngsi-ld:Entity:2".parse::<Identifier>()?;
        let result = batch_operation_result(vec![success], vec![(failed, "invalid")], |detail| {
            ClassifiedReport::public(NgsiLdErrorType::BadRequestData, *detail)
        });
        pretty_assertions::assert_eq!(result.success[0].as_str(), "urn:ngsi-ld:Entity:1");
        pretty_assertions::assert_eq!(result.errors[0].entity_id.as_str(), "urn:ngsi-ld:Entity:2");
        pretty_assertions::assert_eq!(result.errors[0].error.detail, "invalid");
        Ok(())
    }
}
