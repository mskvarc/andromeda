//! HTTP handling for the update attributes entity operation.

use crate::entity::{
    ContextUrl,
    EntityResultExt,
    EntityTenantState,
    ForwardProvisionRequest,
    ForwardingOperation,
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
    Uri,
    Value,
    classify_entity_error,
    collect_proxied_attrs,
    provision_response,
    reject_unknown_attribute_types,
    strip_attributes_from_value,
    update_result::{NotUpdatedReason, update_result_response},
};
use andromeda_federation::{outcome::ForwardedWriteResult, request::TriggeringRequest};
use andromeda_http_extraction::context_target;
use axum::http::{HeaderMap, Method};

pub(crate) async fn update_attrs(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath(id): TypedPath<Uri>,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
    JsonLd {
        value: fragment,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;
    let context = context.merge(body_context);
    reject_unknown_attribute_types(&fragment, &uri)?;

    // Forward to matching context sources
    let fragment_body = serde_json::to_vec(&fragment).with_entity_instance(&uri)?;

    let forwarded = state
        .forwarding
        .forward_provision(
            &target,
            ForwardProvisionRequest {
                resource: ProvisionResource::Entities,
                entity_id: &id,
                entity_type_compact: None,
                attribute_name: None,
                method: Method::PATCH,
                body: Some(&fragment_body),
                context: &context,
                operation: ForwardingOperation::UpdateEntityAttributes,
                path_suffix: Some("/attrs"),
                query_string: None,
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
            },
        )
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        // No matching CSRs — update locally
        let (requested, not_updated) = service.update_attrs(&target, &id, fragment, &context).await.with_entity_instance(&uri)?;
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
    // fragment; whatever remains is updated here.
    let proxied = collect_proxied_attrs(&forwarded);
    let mut local_fragment = fragment;
    strip_attributes_from_value(&mut local_fragment, &proxied);
    let local_update = if local_fragment.as_object().is_some_and(serde_json::Map::is_empty) {
        None
    } else {
        Some(service.update_attrs(&target, &id, local_fragment, &context).await)
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
