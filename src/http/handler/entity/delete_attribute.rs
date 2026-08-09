//! HTTP handling for the delete attribute entity operation.

use crate::http::handler::entity::{
    AttributeName,
    ContextSpec,
    ContextUrl,
    DatasetSelection,
    DatasetSelector,
    DeleteAttrParams,
    ForwardProvisionRequest,
    ForwardingModes,
    ForwardingOperation,
    IncomingVia,
    IntoResponse,
    Json,
    NgsiLdError,
    OriginalUri,
    Response,
    ResultInstanceExt,
    StatusCode,
    TenantState,
    TypedPath,
    Uri,
    ValidatedQuery,
    build_multi_status_body,
    first_forwarded_status,
    has_remote_errors,
};

pub async fn delete_attr(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath((id, attr_id)): TypedPath<(Uri, AttributeName)>,
    ValidatedQuery(params): ValidatedQuery<DeleteAttrParams>,
    IncomingVia(incoming_via): IncomingVia,
) -> Result<Response, NgsiLdError> {
    let service = &state.entities;
    let context: ContextSpec = context.into_inner();
    let delete_all = params.delete_all.unwrap_or(false);
    let dataset_id = params.dataset_id.as_ref();
    // NGSI-LD clause 5.6.5: `deleteAll` wins over `datasetId`, and with neither
    // present the default (no-datasetId) instance is the target.
    let selection = if delete_all {
        DatasetSelection::All
    } else {
        match dataset_id {
            Some(DatasetSelector::Id(dataset_id)) => DatasetSelection::Single(dataset_id),
            Some(DatasetSelector::Default) | None => DatasetSelection::Default,
        }
    };

    // Forward to matching context sources
    let context_url = context.link_contexts().first();
    let path_suffix = format!("/attrs/{attr_id}");

    // Build query string for forwarding
    let mut qs_parts = Vec::new();
    if delete_all {
        qs_parts.push("deleteAll=true".to_string());
    }
    if let Some(ref ds) = params.dataset_id {
        qs_parts.push(format!("datasetId={ds}"));
    }
    let qs = if qs_parts.is_empty() { None } else { Some(qs_parts.join("&")) };

    let forwarded = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            entity_id: &id,
            entity_type_compact: None,
            attribute_name: Some(&attr_id),
            method: reqwest::Method::DELETE,
            body: None,
            context_url,
            context: &context,
            operation: ForwardingOperation::DeleteEntityAttribute,
            path_suffix: Some(&path_suffix),
            query_string: qs.as_deref(),
            incoming_via: incoming_via.as_ref(),
        })
        .await
        .with_instance(&uri)?;

    if forwarded.is_empty() {
        service.delete_attr(&id, &attr_id, selection, &context).await.with_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let modes = ForwardingModes::from_results(&forwarded);
    if modes.has_redirect || modes.has_exclusive {
        let status = StatusCode::from_u16(first_forwarded_status(&forwarded, 204)).unwrap_or(StatusCode::NO_CONTENT);
        return Ok(status.into_response());
    }

    // Inclusive: delete locally and combine results
    let local_result = service.delete_attr(&id, &attr_id, selection, &context).await;

    if local_result.is_ok() && !has_remote_errors(&forwarded) {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let body = build_multi_status_body(&id, local_result.is_ok(), local_result.err().as_ref(), &forwarded);
    Ok((StatusCode::MULTI_STATUS, Json(body)).into_response())
}
