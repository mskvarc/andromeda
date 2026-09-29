//! HTTP handling for the delete attribute entity operation.

use crate::entity::{
    AttributeName,
    ContextSpec,
    ContextUrl,
    DatasetSelection,
    DatasetSelector,
    DeleteAttrParams,
    EntityResultExt,
    EntityTenantState,
    ForwardProvisionRequest,
    ForwardingOperation,
    Identifier,
    IncomingVia,
    IntoResponse,
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
};
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::context_target;
use axum::http::{HeaderMap, Method};

pub(crate) async fn delete_attr(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath((id, attr_id)): TypedPath<(Identifier, AttributeName)>,
    ValidatedQuery(params): ValidatedQuery<DeleteAttrParams>,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
) -> Result<Response, ProblemResponse> {
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
        .forward_provision(
            &target,
            ForwardProvisionRequest {
                resource: ProvisionResource::Entities,
                entity_id: &id,
                entity_type_compact: None,
                attribute_name: Some(&attr_id),
                method: Method::DELETE,
                body: None,
                context: &context,
                operation: ForwardingOperation::DeleteEntityAttribute,
                path_suffix: Some(&path_suffix),
                query_string: qs.as_deref(),
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
            },
        )
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        service
            .delete_attr(&target, &id, &attr_id, selection, &context)
            .await
            .with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // Clause 5.6.5.4: an attribute a proxied registration covers is deleted
    // at the source, never here; an inclusive one is deleted in both places.
    let local = if collect_proxied_attrs(&forwarded).contains(&attr_id) {
        LocalOutcome::NothingRemained
    } else {
        LocalOutcome::processed(service.delete_attr(&target, &id, &attr_id, selection, &context).await, classify_entity_error)
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}
