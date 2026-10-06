//! HTTP handling for the update attributes entity operation.

use crate::{
    entity::{
        EntityPatch,
        EntityResultExt,
        EntityTenantState,
        ForwardProvisionRequest,
        ForwardingOperation,
        Identifier,
        IncomingVia,
        IntoResponse,
        JsonLd,
        LocalOutcome,
        OriginalUri,
        ProblemResponse,
        ProvisionResource,
        Response,
        StatusCode,
        TypedPath,
        ValidatedQuery,
        classify_entity_error,
        collect_proxied_attrs,
        provision_response,
        update_result::{NotUpdatedReason, update_result_response},
    },
    written_payload_admission::admit_written_entity,
};
use andromeda_entity::write_kind::UpdateAttributes;
use andromeda_federation::{forwarded_content::ForwardedContent, outcome::ForwardedWriteResult, request::TriggeringRequest};
use andromeda_http_extraction::context_target;
use andromeda_jsonld::user_context_admission::admit_user_context;
use andromeda_ngsi_ld_request::empty::EmptyParams;
use axum::http::{HeaderMap, Method};

pub(crate) async fn update_attrs(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before the tenant is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(id): TypedPath<Identifier>,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
    JsonLd { body, context }: JsonLd<EntityPatch>,
) -> Result<Response, ProblemResponse> {
    // Clause 5.5.4 judges the payload "according to the @context", so the
    // context is admitted before anything is read under it (clause 5.5.7).
    let context = admit_user_context(state.entity_query.engine(), context).await.with_entity_instance(&uri)?;
    let fragment = body.admitted(&context)?;
    let service = &state.entities;
    let request = ForwardProvisionRequest {
        resource: ProvisionResource::Entities,
        entity_id: &id,
        entity_type_compact: None,
        attribute_name: None,
        method: Method::PATCH,
        context: &context,
        operation: ForwardingOperation::UpdateEntityAttributes,
        path_suffix: Some("/attrs"),
        query_string: None,
        triggering: TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        },
    };
    let plan = state.forwarding.plan_provision(&target, &request).await.with_entity_instance(&uri)?;
    // Clause 5.6.2.4 runs the clause 5.5.4 validation before the forwarding
    // phase, so a fragment this broker refuses reaches no context source and an
    // update left with nothing local is still validated. The registrations the
    // plan sends the fragment to read the document the admission expanded.
    let admitted = admit_written_entity::<UpdateAttributes>(service, &fragment, &context)
        .await
        .with_entity_instance(&uri)?;
    let sent_to_registrations = plan.reads_payload();
    let forwarded = state
        .forwarding
        .execute_provision(plan, &request, Some(ForwardedContent::Entity(admitted.document())))
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        // No matching CSRs — update locally
        let (requested, not_updated) = service.update_attrs(&target, &id, admitted).await.with_entity_instance(&uri)?;
        if not_updated.is_empty() {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        return Ok(update_result_response(
            &requested,
            &not_updated,
            NotUpdatedReason::AttributeNotFound,
            Vec::new(),
        ));
    }

    // Clause 5.6.2.4: the attributes proxied registrations cover leave the
    // fragment; whatever remains is updated here. A fragment no registration
    // read was taken over by none of them.
    let local_update = if sent_to_registrations {
        match admitted.local_remainder(&collect_proxied_attrs(&forwarded)) {
            Some(remainder) => Some(service.update_attrs(&target, &id, remainder).await),
            None => None,
        }
    } else {
        Some(service.update_attrs(&target, &id, admitted).await)
    };

    // Clause 5.6.2.5: the status says "whether all the new Attributes were
    // updated or only some of them". An attribute the entity did not carry
    // makes it "only some" without anything having failed, and that distinction
    // does not survive the flattening below -- `LocalOutcome` keeps only success
    // or failure -- so it is answered here first, with the list of attributes
    // actually updated.
    let (local_requested, local_not_updated) = match &local_update {
        Some(Ok((requested, not_updated))) => (requested.as_slice(), not_updated.as_slice()),
        Some(Err(_)) | None => (&[][..], &[][..]),
    };
    if !local_not_updated.is_empty() && !forwarded.iter().any(ForwardedWriteResult::is_failure) {
        return Ok(update_result_response(
            local_requested,
            local_not_updated,
            NotUpdatedReason::AttributeNotFound,
            Vec::new(),
        ));
    }

    let local = match local_update {
        None => LocalOutcome::NothingRemained,
        Some(result) => LocalOutcome::processed(result, classify_entity_error),
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}
