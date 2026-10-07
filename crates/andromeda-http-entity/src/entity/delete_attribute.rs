//! HTTP handling for the delete attribute entity operation.

use crate::{
    entity::{
        AttributeName,
        ContextSpec,
        ContextUrl,
        DatasetSelection,
        DatasetSelector,
        EntityAttributeDeletionParams,
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
        collect_proxied_attrs,
        local_entity_outcome,
        provision_response,
    },
    forwarded_write_query::{forwarded_type_parameter, forwarded_write_query},
};
use andromeda_entity::write_target::WriteTarget;
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
    ValidatedQuery(params): ValidatedQuery<EntityAttributeDeletionParams>,
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

    let path_suffix = format!("/attrs/{attr_id}");

    let selector = service
        .expand_write_selector(params.entity_type.as_ref(), &context)
        .await
        .with_entity_instance(&uri)?;
    let entity = WriteTarget::new(&id, selector.as_ref());

    let qs = forwarded_write_query([
        delete_all.then(|| ("deleteAll", "true".to_owned())),
        params.dataset_id.as_ref().map(|dataset_id| ("datasetId", dataset_id.to_string())),
        forwarded_type_parameter(uri.query()),
    ]);

    let forwarded = state
        .forwarding
        .forward_provision(
            &target,
            ForwardProvisionRequest {
                resource: ProvisionResource::Entities,
                entity_id: &id,
                entity_type_selector: params.entity_type.as_ref(),
                attribute_name: Some(&attr_id),
                method: Method::DELETE,
                context: &context,
                operation: ForwardingOperation::DeleteEntityAttribute,
                path_suffix: Some(&path_suffix),
                query_string: qs.as_deref(),
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
            },
            None,
        )
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        service
            .delete_attr(&target, entity, &attr_id, selection, &context)
            .await
            .with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // Clause 5.6.5.4: an attribute a proxied registration covers is deleted
    // at the source, never here; an inclusive one is deleted in both places.
    let local = if collect_proxied_attrs(&forwarded).is_empty() {
        local_entity_outcome(service.delete_attr(&target, entity, &attr_id, selection, &context).await, &forwarded)
    } else {
        LocalOutcome::NothingRemained
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}
