//! HTTP handling for the replace attribute entity operation.

use crate::entity::{
    AttributeName,
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
    ReplaceAttrParams,
    Response,
    StatusCode,
    TypedPath,
    Uri,
    ValidatedQuery,
    Value,
    classify_entity_error,
    collect_proxied_attrs,
    provision_response,
    reject_unknown_attribute_node_type,
    selected_dataset_uri,
};
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::tenant_state;
use axum::http::{HeaderMap, Method};

pub(crate) async fn replace_attr(
    tenant_state::ExistingTenantState(state): EntityTenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath((id, attr_id)): TypedPath<(Uri, AttributeName)>,
    ValidatedQuery(params): ValidatedQuery<ReplaceAttrParams>,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
    JsonLd {
        value: fragment,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;
    let dataset_id = selected_dataset_uri(params.dataset_id.as_ref());
    let context = context.merge(body_context);
    reject_unknown_attribute_node_type(&attr_id, &fragment, &uri)?;
    let fragment_body = serde_json::to_vec(&fragment).with_entity_instance(&uri)?;
    let path_suffix = format!("/attrs/{attr_id}");
    let qs = params.dataset_id.as_ref().map(|dataset_id| format!("datasetId={dataset_id}"));

    let forwarded = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            resource: ProvisionResource::Entities,
            entity_id: &id,
            entity_type_compact: None,
            attribute_name: Some(&attr_id),
            method: Method::PUT,
            body: Some(&fragment_body),
            context: &context,
            operation: ForwardingOperation::ReplaceEntityAttribute,
            path_suffix: Some(&path_suffix),
            query_string: qs.as_deref(),
            triggering: TriggeringRequest {
                via: incoming_via.as_ref(),
                headers: Some(&headers),
            },
        })
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        service
            .replace_attr(&id, &attr_id, dataset_id, fragment, &context)
            .await
            .with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // Clause 5.6.19.4: an attribute a proxied registration covers is replaced
    // at the source, never here; an inclusive one is replaced in both places.
    let local = if collect_proxied_attrs(&forwarded).contains(&attr_id) {
        LocalOutcome::NothingRemained
    } else {
        LocalOutcome::processed(service.replace_attr(&id, &attr_id, dataset_id, fragment, &context).await, classify_entity_error)
    };
    provision_response(&id, &forwarded, local, StatusCode::NO_CONTENT.into_response(), &uri)
}
