//! Projection of policy batch outcomes and forwarded write outcomes into
//! clause 5.2.16 wire values.

use crate::classified_report::ClassifiedReport;
use andromeda_federation::outcome::{ForwardedWriteOutcome, TransportFailure};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_problem_details::{
    batch_operation::{BatchEntityError, BatchOperationResult, BatchProblemDetail},
    error_type::NgsiLdErrorType,
    problem_detail::ProblemClassification,
};
use serde::Deserialize;
use serde_json::Value;

/// Project one policy error through its binding-supplied exhaustive classifier.
pub fn batch_entity_error<E>(entity_id: Uri, error: &E, classify: impl FnOnce(&E) -> ClassifiedReport) -> BatchEntityError {
    let report = classify(error);
    BatchEntityError::new(entity_id, report.error_type(), report.detail())
}

/// Project a complete policy batch outcome into the clause 5.2.16 wire envelope.
pub fn batch_operation_result<E>(success: Vec<Uri>, errors: Vec<(Uri, E)>, classify: impl Fn(&E) -> ClassifiedReport) -> BatchOperationResult {
    let errors = errors
        .into_iter()
        .map(|(entity_id, error)| batch_entity_error(entity_id, &error, &classify))
        .collect();
    BatchOperationResult { success, errors }
}

/// The clause 5.2.17 error a forwarded write contributes for `entity_id`, or
/// `None` when the context source completed it.
///
/// A problem detail the source answered with is carried through as it came
/// (RFC 7807 permits any type); any other failure is classified from what
/// happened, with the clause 6.3.17 statuses for the ones that produced no
/// response. Every error names the registration it came from (Table
/// 5.2.17-1 `registrationId`).
#[must_use]
pub fn forwarded_write_error(entity_id: Uri, registration_id: &Uri, outcome: &ForwardedWriteOutcome) -> Option<BatchEntityError> {
    let error = match outcome {
        ForwardedWriteOutcome::Completed { .. } => return None,
        ForwardedWriteOutcome::RemoteHttpFailure { status, body } => match body.as_ref().and_then(|body| BatchProblemDetail::deserialize(body).ok()) {
            Some(problem) => BatchEntityError {
                entity_id,
                registration_id: None,
                error: problem,
            },
            None => BatchEntityError::from_classification(entity_id, classification_for_remote_status(*status), &remote_detail(*status, body.as_ref())),
        },
        ForwardedWriteOutcome::Transport(TransportFailure::Timeout { detail }) => {
            BatchEntityError::from_classification(entity_id, ProblemClassification::GatewayTimeout, detail)
        }
        ForwardedWriteOutcome::Transport(TransportFailure::Unreachable { detail }) => {
            BatchEntityError::from_classification(entity_id, ProblemClassification::BadGateway, detail)
        }
        ForwardedWriteOutcome::LoopDetected => BatchEntityError::from_classification(
            entity_id,
            ProblemClassification::LoopDetected,
            &format!("registration {registration_id} loops back into the request's Via chain"),
        ),
        ForwardedWriteOutcome::NotAdvertised => BatchEntityError::new(
            entity_id,
            NgsiLdErrorType::Conflict,
            &format!("registration {registration_id} covers the data but does not advertise the requested operation"),
        ),
        ForwardedWriteOutcome::UpsertFollowUpNotAdvertised => BatchEntityError::new(
            entity_id,
            NgsiLdErrorType::OperationNotSupported,
            &format!("registration {registration_id} reported the entity exists and advertises no operation to update it"),
        ),
    };
    Some(error.with_registration_id(registration_id.clone())) // clone: the error owns the identifier of the registration it names
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

/// The most informative detail a remote error carries: its `detail`, its
/// `title`, or the bare status.
fn remote_detail(status: u16, body: Option<&Value>) -> String {
    body.and_then(|body| body.get("detail").or_else(|| body.get("title")))
        .and_then(Value::as_str)
        .map_or_else(
            || format!("context source answered HTTP {status}"),
            |detail| format!("context source answered HTTP {status}: {detail}"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_problem_details::{batch_operation::BatchErrorKind, error_type::NgsiLdErrorType};
    use serde_json::json;
    use std::error::Error;

    fn uri(raw: &str) -> Uri {
        raw.parse().expect("valid URI")
    }

    #[test]
    fn a_completed_forward_contributes_no_error() {
        assert!(
            forwarded_write_error(
                uri("urn:ngsi-ld:Entity:1"),
                &uri("urn:ngsi-ld:ContextSourceRegistration:1"),
                &ForwardedWriteOutcome::Completed { status: 204, body: None }
            )
            .is_none()
        );
    }

    #[test]
    fn a_sources_problem_detail_is_carried_through_under_its_registration() {
        let error = forwarded_write_error(
            uri("urn:ngsi-ld:Entity:1"),
            &uri("urn:ngsi-ld:ContextSourceRegistration:1"),
            &ForwardedWriteOutcome::RemoteHttpFailure {
                status: 409,
                body: Some(json!({"type": "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists", "title": "exists", "status": 409})),
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
                uri("urn:ngsi-ld:Entity:1"),
                &uri("urn:ngsi-ld:ContextSourceRegistration:1"),
                &ForwardedWriteOutcome::RemoteHttpFailure {
                    status,
                    body: Some(json!({"message": "not a problem detail"})),
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
                ForwardedWriteOutcome::Transport(TransportFailure::Timeout {
                    detail: "no response within 5000ms".to_string(),
                }),
                ProblemClassification::GatewayTimeout,
            ),
            (
                ForwardedWriteOutcome::Transport(TransportFailure::Unreachable {
                    detail: "connection refused".to_string(),
                }),
                ProblemClassification::BadGateway,
            ),
            (ForwardedWriteOutcome::LoopDetected, ProblemClassification::LoopDetected),
            (ForwardedWriteOutcome::NotAdvertised, ProblemClassification::Category(NgsiLdErrorType::Conflict)),
            (
                ForwardedWriteOutcome::UpsertFollowUpNotAdvertised,
                ProblemClassification::Category(NgsiLdErrorType::OperationNotSupported),
            ),
        ] {
            let error = forwarded_write_error(uri("urn:ngsi-ld:Entity:1"), &registration, &outcome).expect("a failure contributes an error");
            pretty_assertions::assert_eq!(error.error.error_type, BatchErrorKind::Known(expected), "{outcome:?}");
            pretty_assertions::assert_eq!(error.registration_id.as_ref(), Some(&registration));
        }
    }

    #[test]
    fn projection_preserves_successes_failures_and_supplied_classification() -> Result<(), Box<dyn Error>> {
        let success = "urn:ngsi-ld:Entity:1".parse::<Uri>()?;
        let failed = "urn:ngsi-ld:Entity:2".parse::<Uri>()?;
        let result = batch_operation_result(vec![success], vec![(failed, "invalid")], |detail| {
            ClassifiedReport::public(NgsiLdErrorType::BadRequestData, *detail)
        });
        pretty_assertions::assert_eq!(result.success[0].as_str(), "urn:ngsi-ld:Entity:1");
        pretty_assertions::assert_eq!(result.errors[0].entity_id.as_str(), "urn:ngsi-ld:Entity:2");
        pretty_assertions::assert_eq!(result.errors[0].error.detail, "invalid");
        Ok(())
    }
}
