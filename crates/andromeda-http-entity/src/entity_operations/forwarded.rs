//! The distributed half of every batch operation (clauses 5.6.7.4 to
//! 5.6.10.4 and 5.6.20.4): what a handler forwards before its local phase and
//! how the forwarded answers merge into the clause 5.2.16 result.

use crate::{error::EntityResultExt, state::EntityRouteServices};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{batch::admitted_batch::AdmittedBatch, error::EntityError, write_kind::BatchWriteKind};
use andromeda_federation::{
    outcome::ForwardedWriteOutcome,
    provision_batch::{BatchEntity, BatchForwardedResult, BatchProvisionOperation, ForwardBatchRequest},
    request::TriggeringRequest,
};
use andromeda_http_response::{batch_projection::forwarded_write_error, problem_response::ProblemResponse};
use andromeda_jsonld::{batch_contexts::BatchContext, user_context_admission::AdmittedContext};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
use andromeda_ngsi_ld_problem_details::batch_operation::BatchOperationResult;
use axum::http::{StatusCode, Uri as HttpUri};

/// What a batch operation asks the registrations matching it to do.
///
/// The three members travel together because none of them describes the
/// entities: they say which clause 5.6.7 to 5.6.10 operation the sources
/// perform, which of the client's options every forwarded copy repeats, and
/// which request triggered the forwarding (clause 6.3.18).
pub(crate) struct ForwardedBatch<'a> {
    /// The operation the matching registrations perform.
    pub(crate) operation: BatchProvisionOperation,
    /// The client's options, repeated on every forwarded copy.
    pub(crate) query_string: Option<&'a str>,
    /// The triggering request every forwarded copy replays, so a forwarding
    /// loop is detected rather than followed.
    pub(crate) triggering: TriggeringRequest<'a>,
}

/// Forward the admitted entities of a batch to every matching registration,
/// each under the context it was submitted with.
///
/// Each entity is the document its admission expanded, compacted only for a
/// registration it matches, so a batch no registration covers compacts
/// nothing.
pub(crate) async fn forward_entities<W: BatchWriteKind>(
    services: &EntityRouteServices,
    target: &ContextTarget,
    request: ForwardedBatch<'_>,
    admitted: &AdmittedBatch<W>,
    instance: &HttpUri,
) -> Result<Vec<BatchForwardedResult>, ProblemResponse> {
    let ForwardedBatch {
        operation,
        query_string,
        triggering,
    } = request;
    let batch = admitted
        .forwardable()
        .map(|entity| BatchEntity {
            id: entity.id,
            document: Some(entity.document),
            context: entity.context,
        })
        .collect::<Vec<_>>();
    services
        .forwarding
        .forward_batch(
            target,
            ForwardBatchRequest {
                operation,
                entities: &batch,
                query_string,
                triggering,
            },
        )
        .await
        .with_entity_instance(instance)
}

/// Forward a batch of identifiers (clause 5.6.10) to every matching
/// registration, every one under the request's `context`.
pub(crate) async fn forward_identifiers(
    services: &EntityRouteServices,
    target: &ContextTarget,
    ids: &[Identifier],
    context: AdmittedContext,
    triggering: TriggeringRequest<'_>,
    instance: &HttpUri,
) -> Result<Vec<BatchForwardedResult>, ProblemResponse> {
    let context = BatchContext::shared(context.into_spec());
    let batch = ids
        .iter()
        .map(|id| BatchEntity {
            id,
            document: None,
            context: &context,
        })
        .collect::<Vec<_>>();
    services
        .forwarding
        .forward_batch(
            target,
            ForwardBatchRequest {
                operation: BatchProvisionOperation::Delete,
                entities: &batch,
                query_string: None,
                triggering,
            },
        )
        .await
        .with_entity_instance(instance)
}

/// The attributes proxied registrations claim from `entity_id`, whether or
/// not their forward succeeded (clause 4.3.6.3).
fn proxied_attributes(forwarded: &[BatchForwardedResult], entity_id: &Identifier) -> Vec<AttributeName> {
    forwarded
        .iter()
        .filter(|result| result.entity_id == *entity_id)
        .flat_map(|result| result.proxied_attrs.iter())
        .cloned() // clone: the strip reads the names while the admitted entity is mutated
        .collect()
}

/// Narrow the admitted entities to what the local phase processes: each
/// without the attributes proxied registrations took (clauses 5.6.7.4 and
/// following: "any remaining input data"), and without the ones left with no
/// attribute at all.
pub(crate) fn local_remainder<W: BatchWriteKind>(admitted: &mut AdmittedBatch<W>, forwarded: &[BatchForwardedResult]) {
    admitted.retain_local(|entity_id| proxied_attributes(forwarded, entity_id));
}

/// Drop the local not-found of an entity a proxied registration holds: its
/// absence here is not a failure when the deletion landed at the source
/// (clause 5.6.6.4).
pub(crate) fn drop_absent_proxied(errors: &mut Vec<(Identifier, EntityError)>, forwarded: &[BatchForwardedResult]) {
    errors.retain(|(id, error)| {
        let proxied = forwarded
            .iter()
            .any(|result| result.entity_id == *id && matches!(result.mode, ForwardingMode::Exclusive | ForwardingMode::Redirect));
        !(matches!(error, EntityError::NotFound { .. }) && proxied)
    });
}

/// Merge the forwarded answers into the clause 5.2.16 result: every
/// completed forward adds its entity to `success` (once), every failed one a
/// clause 5.2.17 error naming its registration.
pub(crate) fn merge_forwarded(result: &mut BatchOperationResult, forwarded: &[BatchForwardedResult]) {
    for entry in forwarded {
        match forwarded_write_error(entry.entity_id.as_uri().clone(), &entry.registration_id, &entry.outcome) {
            // clone: the error owns the identifier of the entity it is about
            Some(error) => result.errors.push(error),
            None => {
                if !result.success.contains(&entry.entity_id) {
                    result.success.push(entry.entity_id.clone()); // clone: the result owns the identifiers it reports
                }
            }
        }
    }
}

/// The entities a context source reports as created (clause 5.6.8.5: the
/// ones answered 201).
pub(crate) fn created_remotely(forwarded: &[BatchForwardedResult]) -> impl Iterator<Item = &Identifier> {
    forwarded.iter().filter_map(|entry| match entry.outcome {
        ForwardedWriteOutcome::Completed { status, .. } if status == StatusCode::CREATED.as_u16() => Some(&entry.entity_id),
        ForwardedWriteOutcome::Completed { .. }
        | ForwardedWriteOutcome::RemoteHttpFailure { .. }
        | ForwardedWriteOutcome::Transport(_)
        | ForwardedWriteOutcome::LoopDetected
        | ForwardedWriteOutcome::NotAdvertised
        | ForwardedWriteOutcome::UpsertFollowUpNotAdvertised => None,
    })
}

#[cfg(test)]
mod tests {
    use crate::entity_operations::forwarded::{created_remotely, drop_absent_proxied, merge_forwarded, proxied_attributes};
    use andromeda_entity::error::EntityError;
    use andromeda_federation::{answer_body::AnswerBody, outcome::ForwardedWriteOutcome, provision_batch::BatchForwardedResult};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
    use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
    use andromeda_ngsi_ld_problem_details::batch_operation::BatchOperationResult;

    fn uri(raw: &str) -> Identifier {
        raw.parse().expect("valid URI")
    }

    fn result(entity: &str, mode: ForwardingMode, outcome: ForwardedWriteOutcome, proxied: &[&str]) -> BatchForwardedResult {
        BatchForwardedResult {
            registration_id: uri("urn:ngsi-ld:csr:1"),
            entity_id: uri(entity),
            mode,
            outcome,
            proxied_attrs: proxied.iter().map(|name| name.parse::<AttributeName>().expect("valid name")).collect(),
        }
    }

    /// A source's bodiless success with `status`.
    const fn completed(status: u16) -> ForwardedWriteOutcome {
        ForwardedWriteOutcome::Completed {
            status,
            body: AnswerBody::Absent,
        }
    }

    fn names(raw: &[&str]) -> Vec<AttributeName> {
        raw.iter().map(|name| name.parse::<AttributeName>().expect("valid name")).collect()
    }

    /// Clause 4.3.6.3: a proxied registration owns its attributes whether or
    /// not its forward succeeded, and an inclusive one claims none.
    #[test]
    fn an_entity_loses_every_attribute_its_proxied_registrations_claim_and_nothing_else() {
        let forwarded = [
            result("urn:ngsi-ld:Vehicle:1", ForwardingMode::Exclusive, completed(204), &["speed"]),
            result(
                "urn:ngsi-ld:Vehicle:2",
                ForwardingMode::Redirect,
                ForwardedWriteOutcome::NotAdvertised,
                &["speed", "brandName"],
            ),
            result("urn:ngsi-ld:Vehicle:3", ForwardingMode::Inclusive, completed(204), &[]),
        ];
        pretty_assertions::assert_eq!(proxied_attributes(&forwarded, &uri("urn:ngsi-ld:Vehicle:1")), names(&["speed"]));
        pretty_assertions::assert_eq!(proxied_attributes(&forwarded, &uri("urn:ngsi-ld:Vehicle:2")), names(&["speed", "brandName"]));
        pretty_assertions::assert_eq!(proxied_attributes(&forwarded, &uri("urn:ngsi-ld:Vehicle:3")), names(&[]));
        pretty_assertions::assert_eq!(proxied_attributes(&forwarded, &uri("urn:ngsi-ld:Vehicle:4")), names(&[]));
    }

    #[test]
    fn forwarded_answers_merge_keyed_by_registration_and_list_each_success_once() {
        let mut merged = BatchOperationResult {
            success: vec![uri("urn:ngsi-ld:Vehicle:1")],
            errors: Vec::new(),
        };
        let forwarded = [
            result("urn:ngsi-ld:Vehicle:1", ForwardingMode::Inclusive, completed(204), &[]),
            result("urn:ngsi-ld:Vehicle:2", ForwardingMode::Redirect, completed(201), &["speed"]),
            result(
                "urn:ngsi-ld:Vehicle:3",
                ForwardingMode::Redirect,
                ForwardedWriteOutcome::NotAdvertised,
                &["speed"],
            ),
        ];
        merge_forwarded(&mut merged, &forwarded);
        pretty_assertions::assert_eq!(merged.success, vec![uri("urn:ngsi-ld:Vehicle:1"), uri("urn:ngsi-ld:Vehicle:2")]);
        pretty_assertions::assert_eq!(merged.errors.len(), 1);
        pretty_assertions::assert_eq!(merged.errors[0].entity_id.as_str(), "urn:ngsi-ld:Vehicle:3");
        pretty_assertions::assert_eq!(merged.errors[0].registration_id, Some(uri("urn:ngsi-ld:csr:1")));
        pretty_assertions::assert_eq!(created_remotely(&forwarded).collect::<Vec<_>>(), vec![&uri("urn:ngsi-ld:Vehicle:2")]);
    }

    #[test]
    fn a_local_not_found_is_dropped_only_for_an_entity_a_proxied_source_holds() {
        let forwarded = [
            result("urn:ngsi-ld:Vehicle:1", ForwardingMode::Redirect, completed(204), &[]),
            result("urn:ngsi-ld:Vehicle:2", ForwardingMode::Inclusive, completed(204), &[]),
        ];
        let mut errors = vec![
            (uri("urn:ngsi-ld:Vehicle:1"), EntityError::not_found(&uri("urn:ngsi-ld:Vehicle:1"))),
            (uri("urn:ngsi-ld:Vehicle:2"), EntityError::not_found(&uri("urn:ngsi-ld:Vehicle:2"))),
            (uri("urn:ngsi-ld:Vehicle:3"), EntityError::not_found(&uri("urn:ngsi-ld:Vehicle:3"))),
        ];
        drop_absent_proxied(&mut errors, &forwarded);
        pretty_assertions::assert_eq!(
            errors.iter().map(|(id, _)| id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:2", "urn:ngsi-ld:Vehicle:3"]
        );
    }
}
