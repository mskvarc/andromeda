//! HTTP handling for temporal delete attribute (clause 5.6.13).

use crate::temporal::{
    AttributeName,
    ContextUrl,
    DeleteAttrParams,
    DeleteScope,
    ForwardProvisionRequest,
    ForwardingOperation,
    HeaderMap,
    Identifier,
    IncomingVia,
    IntoResponse,
    LocalOutcome,
    Method,
    OriginalUri,
    ProblemResponse,
    ProvisionResource,
    Response,
    ResultInstanceExt,
    StatusCode,
    TenantState,
    TriggeringRequest,
    TypedPath,
    ValidatedQuery,
    collect_proxied_attrs,
    local_temporal_outcome,
    provision_response,
};
use andromeda_http_extraction::context_target;

pub async fn delete_attr(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath((id, attr_id)): TypedPath<(Identifier, AttributeName)>,
    ValidatedQuery(params): ValidatedQuery<DeleteAttrParams>,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    let context = context.into_inner();
    let delete_all = params.delete_all.unwrap_or(false);
    let scope = if delete_all { DeleteScope::All } else { DeleteScope::Single };

    if params.local.unwrap_or(false) {
        service
            .delete_attr(&target, &id, &attr_id, params.dataset_id.as_ref(), scope, &context)
            .await
            .with_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let path_suffix = format!("/attrs/{attr_id}");
    let mut query_parts = Vec::new();
    if delete_all {
        query_parts.push("deleteAll=true".to_string());
    }
    if let Some(dataset_id) = &params.dataset_id {
        query_parts.push(format!("datasetId={dataset_id}"));
    }
    let query_string = if query_parts.is_empty() { None } else { Some(query_parts.join("&")) };
    let forwarded = state
        .forwarding
        .forward_provision(
            &target,
            ForwardProvisionRequest {
                resource: ProvisionResource::TemporalEntities,
                entity_id: &id,
                entity_type_selector: None,
                attribute_name: Some(&attr_id),
                method: Method::DELETE,
                context: &context,
                operation: ForwardingOperation::DeleteTemporalAttribute,
                path_suffix: Some(&path_suffix),
                query_string: query_string.as_deref(),
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
            },
            None,
        )
        .await
        .with_instance(&uri)?;

    // Clause 5.6.13.4: an attribute a proxied registration covers is deleted
    // at the source and needs no further processing; an inclusive one is
    // deleted in both places.
    let local = if collect_proxied_attrs(&forwarded).is_empty() {
        local_temporal_outcome(
            service.delete_attr(&target, &id, &attr_id, params.dataset_id.as_ref(), scope, &context).await,
            &forwarded,
        )
    } else {
        LocalOutcome::NothingRemained
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}
