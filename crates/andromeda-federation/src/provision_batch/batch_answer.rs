//! Reading a forwarded batch request's answer entity by entity (clauses
//! 5.6.7.5 and following).

use crate::{answer_body::AnswerBody, outcome::ForwardedWriteOutcome};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_problem_details::batch_operation::{BatchErrorKind, BatchOperationResult};
use std::collections::HashMap;

/// The status a reported entity error stands for when its type names none
/// (RFC 7807 clause 3.1 makes `status` optional).
const UNSTATED_STATUS: u16 = 500;

/// The per-entity outcomes a forwarded batch request answered with
/// (clauses 5.6.7.5 and following): a clause 5.2.16 body is read entity by
/// entity, and a bodiless success, a success whose body is no batch result,
/// or any failure applies to every entity of the array.
pub(super) fn batch_outcomes(ids: &[Identifier], outcome: &ForwardedWriteOutcome) -> Vec<(Identifier, ForwardedWriteOutcome)> {
    let ForwardedWriteOutcome::Completed { status, body } = outcome else {
        return every(ids, outcome);
    };
    let Some(result) = body.decoded::<BatchOperationResult>() else {
        return every(ids, outcome);
    };
    let listed_errors = result
        .errors
        .into_iter()
        .filter_map(|error| {
            let id = error.entity_id.as_str().parse::<Identifier>().ok()?;
            let status = match &error.error.error_type {
                BatchErrorKind::Known(classification) => classification.status_code().as_u16(),
                BatchErrorKind::Other(_) => UNSTATED_STATUS,
            };
            Some((
                id,
                ForwardedWriteOutcome::RemoteHttpFailure {
                    status,
                    body: AnswerBody::Problem(error.error),
                },
            ))
        })
        .collect::<HashMap<_, _>>();
    ids.iter()
        .map(|id| {
            let outcome = listed_errors.get(id).cloned().unwrap_or(ForwardedWriteOutcome::Completed {
                status: *status,
                body: AnswerBody::Absent,
            });
            (id.clone(), outcome)
        })
        .collect()
}

/// `outcome` for every entity of `ids`.
fn every(ids: &[Identifier], outcome: &ForwardedWriteOutcome) -> Vec<(Identifier, ForwardedWriteOutcome)> {
    ids.iter().map(|id| (id.clone(), outcome.clone())).collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        answer_body::AnswerBody,
        outcome::{ForwardedWriteOutcome, TransportFailure},
        provision_batch::{batch_answer::batch_outcomes, registration_fixture::uri},
    };
    use serde_json::json;
    use std::{collections::HashMap, sync::Arc, time::Duration};

    #[test]
    fn a_forwarded_batch_answer_is_read_entity_by_entity() {
        let ids = [uri("urn:ngsi-ld:Vehicle:1"), uri("urn:ngsi-ld:Vehicle:2")];
        let multi_status = ForwardedWriteOutcome::Completed {
            status: 207,
            body: AnswerBody::Text(Arc::from(
                json!({
                    "success": ["urn:ngsi-ld:Vehicle:2"],
                    "errors": [{"entityId": "urn:ngsi-ld:Vehicle:1", "error": {"status": 409, "type": "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists", "title": "exists"}}]
                })
                .to_string()
                .into_bytes(),
            )),
        };
        let outcomes = batch_outcomes(&ids, &multi_status).into_iter().collect::<HashMap<_, _>>();
        assert!(matches!(outcomes[&ids[0]], ForwardedWriteOutcome::RemoteHttpFailure { status: 409, .. }));
        assert!(matches!(outcomes[&ids[1]], ForwardedWriteOutcome::Completed { status: 207, .. }));

        let bodiless = batch_outcomes(
            &ids,
            &ForwardedWriteOutcome::Completed {
                status: 204,
                body: AnswerBody::Absent,
            },
        );
        assert!(
            bodiless
                .iter()
                .all(|(_, outcome)| matches!(outcome, ForwardedWriteOutcome::Completed { status: 204, .. }))
        );

        let failed = batch_outcomes(
            &ids,
            &ForwardedWriteOutcome::Transport(TransportFailure::TimedOut {
                after: Duration::from_millis(1),
            }),
        );
        pretty_assertions::assert_eq!(failed.len(), 2);
        assert!(failed.iter().all(|(_, outcome)| matches!(outcome, ForwardedWriteOutcome::Transport(_))));
    }
}
