//! HTTP handling for the partial update attribute entity operation.

use crate::entity::{
    AttributeName,
    ContextUrl,
    DatasetSelection,
    EntityResultExt,
    EntityTenantState,
    ForwardProvisionRequest,
    ForwardingModes,
    ForwardingOperation,
    IncomingVia,
    IntoResponse,
    JsonLd,
    OriginalUri,
    PartialUpdateAttrParams,
    ProblemResponse,
    Response,
    StatusCode,
    TypedPath,
    Uri,
    ValidatedQuery,
    Value,
    first_forwarded_status,
    selected_dataset_uri,
};
use andromeda_http_extraction::tenant_state;
use axum::http::Method;

pub(crate) async fn partial_update_attr(
    tenant_state::ExistingTenantState(state): EntityTenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath((id, attr_id)): TypedPath<(Uri, AttributeName)>,
    ValidatedQuery(params): ValidatedQuery<PartialUpdateAttrParams>,
    IncomingVia(incoming_via): IncomingVia,
    JsonLd {
        value: fragment,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;
    let dataset_id = selected_dataset_uri(params.dataset_id.as_ref());
    let context = context.merge(body_context);

    // Forward to matching context sources
    let context_url = context.link_contexts().first();
    let fragment_body = serde_json::to_vec(&fragment).with_entity_instance(&uri)?;
    let path_suffix = format!("/attrs/{attr_id}");
    let qs = params.dataset_id.as_ref().map(|dataset_id| format!("datasetId={dataset_id}"));

    let forwarded = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            entity_id: &id,
            entity_type_compact: None,
            attribute_name: Some(&attr_id),
            method: Method::PATCH,
            body: Some(&fragment_body),
            context_url,
            context: &context,
            operation: ForwardingOperation::PartialUpdateAttribute,
            path_suffix: Some(&path_suffix),
            query_string: qs.as_deref(),
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        // No matching CSRs — update locally
        service
            .partial_update_attr(&id, &attr_id, dataset_id, fragment, &context)
            .await
            .with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let modes = ForwardingModes::from_results(&forwarded);
    if modes.has_redirect || modes.has_exclusive {
        // Per spec 4.3.6.3: exclusive attributes shall not be held locally.
        if modes.has_exclusive
            && let Err(e) = service.delete_attr(&id, &attr_id, DatasetSelection::All, &context).await
        {
            tracing::warn!(error = %e, entity_id = %id, attr_id = %attr_id, "Failed to delete exclusive attribute locally");
        }
        let status = StatusCode::from_u16(first_forwarded_status(&forwarded, 204)).unwrap_or(StatusCode::NO_CONTENT);
        return Ok(status.into_response());
    }

    // Inclusive: apply locally and return 204.
    // Local errors are non-fatal — the remote already succeeded.
    if let Err(e) = service.partial_update_attr(&id, &attr_id, dataset_id, fragment, &context).await {
        tracing::warn!(error = %e, entity_id = %id, attr_id = %attr_id, "Failed to apply inclusive partial_update_attr locally");
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}
