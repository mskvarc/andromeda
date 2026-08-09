//! HTTP handling for the merge entity operation.

use crate::entity::{
    ContextUrl,
    EntityPatch,
    EntityResultExt,
    EntityTenantState,
    ForwardProvisionRequest,
    ForwardingModes,
    ForwardingOperation,
    IncomingVia,
    IntoResponse,
    JsonLd,
    LocalOnlyParams,
    OriginalUri,
    ProblemResponse,
    Response,
    StatusCode,
    TypedPath,
    Uri,
    ValidatedQuery,
    collect_forwarded_exclusive_attrs,
    first_forwarded_status,
};

pub(crate) async fn merge(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): EntityTenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath(id): TypedPath<Uri>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    IncomingVia(incoming_via): IncomingVia,
    JsonLd {
        value: patch,
        context: body_context,
    }: JsonLd<EntityPatch>,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;
    let context = context.merge(body_context);
    let is_local = params.local.unwrap_or(false);

    if is_local {
        service.merge(&id, &patch, &context).await.with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let context_url = context.link_contexts().first();
    let patch_body = serde_json::to_vec(&patch).with_entity_instance(&uri)?;

    let forwarded = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            entity_id: &id,
            entity_type_compact: None,
            attribute_name: None,
            method: axum::http::Method::PATCH,
            body: Some(&patch_body),
            context_url,
            context: &context,
            operation: ForwardingOperation::MergeEntity,
            path_suffix: None,
            query_string: None,
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        service.merge(&id, &patch, &context).await.with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let modes = ForwardingModes::from_results(&forwarded);

    if modes.has_redirect {
        let status = StatusCode::from_u16(first_forwarded_status(&forwarded, 204)).unwrap_or(StatusCode::NO_CONTENT);
        return Ok(status.into_response());
    }

    // Per spec 4.3.6.3: exclusive attributes shall not be held locally.
    let exclusive_attrs = collect_forwarded_exclusive_attrs(&forwarded);
    let mut patch = patch;
    if !exclusive_attrs.is_empty() && !patch.strip_exclusive_attrs(&exclusive_attrs) {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    service.merge(&id, &patch, &context).await.with_entity_instance(&uri)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
