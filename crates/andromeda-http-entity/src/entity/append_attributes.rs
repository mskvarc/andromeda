//! HTTP handling for the append attributes entity operation.

use crate::{
    entity::{
        AppendParams,
        AttributeName,
        EntityResultExt,
        EntityTenantState,
        ForwardProvisionRequest,
        ForwardingOperation,
        Identifier,
        IntoResponse,
        JsonLdDocument,
        LocalOutcome,
        NgsiLdRequestEnvelope,
        OverwritePolicy,
        ProblemResponse,
        ProvisionResource,
        RequestOverwritePolicy,
        Response,
        StatusCode,
        TypedPath,
        ValidatedQuery,
        Value,
        classify_entity_error,
        collect_proxied_attrs,
        provision_response,
        strip_attributes_from_value,
        update_result::{NotUpdatedReason, update_result_response},
    },
    written_payload_admission::admit_written_entity,
};
use andromeda_entity::write_kind::AppendAttributes;
use andromeda_federation::{
    outcome::{ForwardedWriteOutcome, ForwardedWriteResult},
    request::TriggeringRequest,
};
use andromeda_http_extraction::context_target;
use axum::http::Method;

pub(crate) async fn append_attrs(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    TypedPath(id): TypedPath<Identifier>,
    ValidatedQuery(params): ValidatedQuery<AppendParams>,
    JsonLdDocument { document: fragment, context }: JsonLdDocument,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;
    // Clause 5.6.3.4 runs the clause 5.5.4 validation before the forwarding
    // phase, so a fragment this broker refuses reaches no context source and an
    // append left with nothing local is still validated.
    // clone: the admission expands its own copy, while the forwarding phase and
    // the narrowing after it still read the compact fragment
    let admitted = admit_written_entity::<AppendAttributes>(service, fragment.clone(), &context)
        .await
        .with_entity_instance(&uri)?;
    let no_overwrite = params.overwrite_policy().with_entity_instance(&uri)? == RequestOverwritePolicy::NoOverwrite;
    let overwrite = if no_overwrite {
        OverwritePolicy::Preserve
    } else {
        OverwritePolicy::Overwrite
    };

    // Forward to matching context sources
    let qs = if no_overwrite { "options=noOverwrite" } else { "" };

    let forwarded = state
        .forwarding
        .forward_provision(
            &target,
            ForwardProvisionRequest {
                resource: ProvisionResource::Entities,
                entity_id: &id,
                entity_type_compact: None,
                attribute_name: None,
                method: Method::POST,
                body: Some(&fragment),
                context: &context,
                operation: ForwardingOperation::AppendEntityAttributes,
                path_suffix: Some("/attrs"),
                query_string: if qs.is_empty() { None } else { Some(qs) },
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
            },
        )
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        // No matching CSRs — append locally
        let (all_attrs, not_appended) = service.append_attrs(&target, &id, admitted, overwrite).await.with_entity_instance(&uri)?;

        return Ok(append_outcome_response(&all_attrs, &not_appended, Vec::new(), ForwardedCompleteness::Complete));
    }

    // Clause 5.6.3.4: the attributes proxied registrations cover leave the
    // fragment; whatever remains is appended here.
    let proxied = collect_proxied_attrs(&forwarded);
    let mut local_fragment = fragment;
    let strip = strip_attributes_from_value(&mut local_fragment, &proxied);
    let local_append = match service.admit_local_remainder(admitted, strip, local_fragment, &context).await {
        Some(remainder) => Some(async { service.append_attrs(&target, &id, remainder?, overwrite).await }.await),
        None => None,
    };
    // Clause 5.6.3.5: the status says "whether all the new Attributes were
    // appended or only some of them", and Table 6.6.3.1-2 reserves 204 for
    // "all the Attributes were appended successfully". Two things make it "only
    // some" without anything having failed: an attribute the local part refused
    // under `noOverwrite`, and a context source that answered 207 for the part
    // forwarded to it. Neither survives the flattening below -- `LocalOutcome`
    // keeps only success or failure, and a 207 is a success to
    // `ForwardedWriteResult::is_failure` -- so both are answered here first.
    let (local_appended, local_not_appended) = match &local_append {
        Some(Ok((all_attrs, not_appended))) => (all_attrs.as_slice(), not_appended.as_slice()),
        Some(Err(_)) | None => (&[][..], &[][..]),
    };
    let forwarded_completeness = forwarded_completeness(&forwarded);
    let partly_appended = !local_not_appended.is_empty() || forwarded_completeness == ForwardedCompleteness::Partial;
    if partly_appended && !forwarded.iter().any(ForwardedWriteResult::is_failure) {
        let remote_not_appended = forwarded_not_appended(&forwarded);
        return Ok(append_outcome_response(
            local_appended,
            local_not_appended,
            remote_not_appended,
            forwarded_completeness,
        ));
    }
    let local = match local_append {
        None => LocalOutcome::NothingRemained,
        Some(result) => LocalOutcome::processed(result, classify_entity_error),
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}

/// The clause 5.6.3.5 answer to an append: 204 when every attribute landed,
/// and otherwise the clause 5.2.18 `UpdateResult` naming what did and did not.
///
/// Table 6.6.3.1-2 pairs the two: 204 "All the Attributes were appended
/// successfully", 207 "Only the Attributes included in the response payload
/// body were successfully appended".
fn append_outcome_response(
    requested: &[AttributeName],
    not_appended: &[AttributeName],
    remote_not_appended: Vec<Value>,
    forwarded: ForwardedCompleteness,
) -> Response {
    if not_appended.is_empty() && forwarded == ForwardedCompleteness::Complete {
        return StatusCode::NO_CONTENT.into_response();
    }
    update_result_response(requested, not_appended, NotUpdatedReason::AlreadyExists, remote_not_appended)
}

/// Whether the forwarded parts of an append each landed in full.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ForwardedCompleteness {
    /// Every contacted source appended everything it was sent.
    Complete,
    /// At least one source answered that it appended only part of it.
    Partial,
}

/// Read the sources' own verdict on whether they appended everything.
///
/// Table 6.6.3.1-2 makes 207 exactly the statement "only the Attributes
/// included in the response payload body were successfully appended", so the
/// status decides this even when a source omits the `UpdateResult` body the
/// table pairs with it. What must not happen either way is relaying that as
/// 204, which asserts the opposite.
fn forwarded_completeness(forwarded: &[ForwardedWriteResult]) -> ForwardedCompleteness {
    let partial = forwarded.iter().any(|result| match &result.outcome {
        ForwardedWriteOutcome::Completed { status, .. } => *status == StatusCode::MULTI_STATUS.as_u16(),
        ForwardedWriteOutcome::RemoteHttpFailure { .. }
        | ForwardedWriteOutcome::Transport(_)
        | ForwardedWriteOutcome::LoopDetected
        | ForwardedWriteOutcome::NotAdvertised
        | ForwardedWriteOutcome::UpsertFollowUpNotAdvertised => false,
    });
    if partial {
        ForwardedCompleteness::Partial
    } else {
        ForwardedCompleteness::Complete
    }
}

/// The `notUpdated` entries the forwarded context sources reported.
///
/// Table 6.6.3.1-2 gives a context source's own 207 the same clause 5.2.18
/// `UpdateResult` body this operation answers with, so a source that appended
/// only part of what it was sent names the part it refused. Those entries
/// belong in this broker's answer: relaying a source's "only some" as 204 would
/// assert the opposite of what the source said.
fn forwarded_not_appended(forwarded: &[ForwardedWriteResult]) -> Vec<Value> {
    forwarded
        .iter()
        .filter_map(|result| match &result.outcome {
            ForwardedWriteOutcome::Completed { status, body } if *status == StatusCode::MULTI_STATUS.as_u16() => body.as_ref(),
            ForwardedWriteOutcome::Completed { .. }
            | ForwardedWriteOutcome::RemoteHttpFailure { .. }
            | ForwardedWriteOutcome::Transport(_)
            | ForwardedWriteOutcome::LoopDetected
            | ForwardedWriteOutcome::NotAdvertised
            | ForwardedWriteOutcome::UpsertFollowUpNotAdvertised => None,
        })
        .filter_map(|body| body.get("notUpdated").and_then(Value::as_array))
        .flat_map(|entries| entries.iter().cloned()) // clone: the answer owns the entries the forwarded bodies still hold
        .collect()
}
