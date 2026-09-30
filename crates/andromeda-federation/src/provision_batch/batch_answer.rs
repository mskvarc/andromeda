//! Reading a forwarded batch request's answer entity by entity (clauses
//! 5.6.7.5 and following).

use crate::outcome::ForwardedWriteOutcome;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use serde_json::Value;
use std::collections::HashMap;

/// The per-entity outcomes a forwarded batch request answered with
/// (clauses 5.6.7.5 and following): a clause 5.2.16 body is read entity by
/// entity, an array of identifiers names the successes, and a bodiless
/// success or any failure applies to every entity of the array.
pub(super) fn batch_outcomes(ids: &[Identifier], outcome: &ForwardedWriteOutcome) -> Vec<(Identifier, ForwardedWriteOutcome)> {
    let ForwardedWriteOutcome::Completed { status, body: Some(body) } = outcome else {
        return ids
            .iter()
            .map(|id| (id.clone(), outcome.clone())) // clone: every entity owns its identifier and the outcome it shares
            .collect();
    };
    let listed_errors = body
        .get("errors")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|error| {
            let id = error.get("entityId")?.as_str()?.parse::<Identifier>().ok()?;
            let problem = error.get("error")?;
            let status = problem
                .get("status")
                .and_then(Value::as_u64)
                .and_then(|status| u16::try_from(status).ok())
                .unwrap_or(500);
            Some((
                id,
                ForwardedWriteOutcome::RemoteHttpFailure {
                    status,
                    body: Some(problem.clone()), // clone: the per-entity outcome owns the problem detail reported for it
                },
            ))
        })
        .collect::<HashMap<_, _>>();
    ids.iter()
        .map(|id| {
            let outcome = listed_errors
                .get(id)
                .cloned()
                .unwrap_or(ForwardedWriteOutcome::Completed { status: *status, body: None }); // clone: the result owns its outcome
            (id.clone(), outcome) // clone: the result owns its identifier
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        outcome::{ForwardedWriteOutcome, TransportFailure},
        provision_batch::{batch_answer::batch_outcomes, registration_fixture::uri},
    };
    use serde_json::json;
    use std::{collections::HashMap, time::Duration};

    #[test]
    fn a_forwarded_batch_answer_is_read_entity_by_entity() {
        let ids = [uri("urn:ngsi-ld:Vehicle:1"), uri("urn:ngsi-ld:Vehicle:2")];
        let multi_status = ForwardedWriteOutcome::Completed {
            status: 207,
            body: Some(json!({
                "success": ["urn:ngsi-ld:Vehicle:2"],
                "errors": [{"entityId": "urn:ngsi-ld:Vehicle:1", "error": {"status": 409, "type": "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists", "title": "exists"}}]
            })),
        };
        let outcomes = batch_outcomes(&ids, &multi_status).into_iter().collect::<HashMap<_, _>>();
        assert!(matches!(outcomes[&ids[0]], ForwardedWriteOutcome::RemoteHttpFailure { status: 409, .. }));
        assert!(matches!(outcomes[&ids[1]], ForwardedWriteOutcome::Completed { status: 207, .. }));

        let bodiless = batch_outcomes(&ids, &ForwardedWriteOutcome::Completed { status: 204, body: None });
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
