//! HTTP handling for the replace entity operation.

use crate::entity::{
    EntityPatch,
    EntityResultExt,
    EntityTenantState,
    ForwardProvisionRequest,
    ForwardingOperation,
    IntoResponse,
    JsonLd,
    LocalOnlyParams,
    LocalOutcome,
    NgsiLdRequestEnvelope,
    ProblemResponse,
    ProvisionResource,
    Response,
    StatusCode,
    TypedPath,
    Uri,
    ValidatedQuery,
    classify_entity_error,
    collect_proxied_attrs,
    provision_response,
    reject_unknown_attribute_types,
};
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::context_target;
use axum::http::Method;

pub(crate) async fn update(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via,
        headers,
    }: NgsiLdRequestEnvelope,
    TypedPath(id): TypedPath<Uri>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    JsonLd {
        value: patch,
        context: body_context,
    }: JsonLd<EntityPatch>,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;
    let context = context.merge(body_context);
    let is_local = params.local.unwrap_or(false);
    let patch_value = serde_json::to_value(&patch).with_entity_instance(&uri)?;
    reject_unknown_attribute_types(&patch_value, &uri)?;

    if is_local {
        service.replace_entity(&target, &id, &patch, &context).await.with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    let patch_body = serde_json::to_vec(&patch_value).with_entity_instance(&uri)?;

    let forwarded = state
        .forwarding
        .forward_provision(
            &target,
            ForwardProvisionRequest {
                resource: ProvisionResource::Entities,
                entity_id: &id,
                entity_type_compact: None,
                attribute_name: None,
                method: Method::PUT,
                body: Some(&patch_body),
                context: &context,
                operation: ForwardingOperation::ReplaceEntity,
                path_suffix: None,
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
        service.replace_entity(&target, &id, &patch, &context).await.with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // Clause 5.6.18.4: the attributes proxied registrations cover leave the
    // fragment; whatever remains replaces the entity here.
    let proxied = collect_proxied_attrs(&forwarded);
    let mut patch = patch;
    let local = if patch.strip_attributes(&proxied) {
        LocalOutcome::processed(service.replace_entity(&target, &id, &patch, &context).await, classify_entity_error)
    } else {
        LocalOutcome::NothingRemained
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}
