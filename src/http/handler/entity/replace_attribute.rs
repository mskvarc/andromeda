//! HTTP handling for the replace attribute entity operation.

use crate::http::handler::entity::{
    AndromedaError,
    AttributeName,
    ContextUrl,
    DatasetSelection,
    ForwardProvisionRequest,
    ForwardingModes,
    ForwardingOperation,
    IncomingVia,
    IntoResponse,
    JsonLd,
    NgsiLdError,
    OriginalUri,
    ReplaceAttrParams,
    Response,
    ResultInstanceExt,
    StatusCode,
    TenantState,
    TypedPath,
    Uri,
    ValidatedQuery,
    Value,
    first_forwarded_status,
    selected_dataset_uri,
};

pub async fn replace_attr(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath((id, attr_id)): TypedPath<(Uri, AttributeName)>,
    ValidatedQuery(params): ValidatedQuery<ReplaceAttrParams>,
    IncomingVia(incoming_via): IncomingVia,
    JsonLd {
        value: fragment,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;
    let dataset_id = selected_dataset_uri(params.dataset_id.as_ref());
    let context = context.merge(body_context);

    let context_url = context.link_contexts().first();
    let fragment_body =
        serde_json::to_vec(&fragment).map_err(|e| AndromedaError::internal(format!("Failed to serialize fragment: {}", e)).with_instance(&uri))?;
    let path_suffix = format!("/attrs/{attr_id}");
    let qs = params.dataset_id.as_ref().map(|dataset_id| format!("datasetId={dataset_id}"));

    let forwarded = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            entity_id: &id,
            entity_type_compact: None,
            attribute_name: Some(&attr_id),
            method: reqwest::Method::PUT,
            body: Some(&fragment_body),
            context_url,
            context: &context,
            operation: ForwardingOperation::ReplaceEntityAttribute,
            path_suffix: Some(&path_suffix),
            query_string: qs.as_deref(),
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .with_instance(&uri)?;

    if forwarded.is_empty() {
        service.replace_attr(&id, &attr_id, dataset_id, fragment, &context).await.with_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let modes = ForwardingModes::from_results(&forwarded);
    if modes.has_redirect || modes.has_exclusive {
        if modes.has_exclusive
            && let Err(error) = service.delete_attr(&id, &attr_id, DatasetSelection::All, &context).await
        {
            tracing::warn!(error = %error, entity_id = %id, attr_id = %attr_id, "Failed to delete exclusive attribute locally");
        }
        let status = StatusCode::from_u16(first_forwarded_status(&forwarded, 204)).unwrap_or(StatusCode::NO_CONTENT);
        return Ok(status.into_response());
    }

    if let Err(error) = service.replace_attr(&id, &attr_id, dataset_id, fragment, &context).await {
        tracing::warn!(error = %error, entity_id = %id, attr_id = %attr_id, "Failed to apply inclusive replace_attr locally");
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}
