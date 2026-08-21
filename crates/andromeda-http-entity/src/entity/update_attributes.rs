//! HTTP handling for the update attributes entity operation.

use crate::entity::{
    ContextUrl,
    EntityResultExt,
    EntityTenantState,
    ForwardProvisionRequest,
    ForwardingOperation,
    IncomingVia,
    IntoResponse,
    Json,
    JsonLd,
    LocalOutcome,
    OriginalUri,
    ProblemResponse,
    Response,
    StatusCode,
    TypedPath,
    Uri,
    Value,
    collect_proxied_attrs,
    provision_response,
    strip_attributes_from_value,
};
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::tenant_state;
use axum::http::{HeaderMap, Method};

pub(crate) async fn update_attrs(
    tenant_state::ExistingTenantState(state): EntityTenantState,
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

    // Forward to matching context sources
    let fragment_body = serde_json::to_vec(&fragment).with_entity_instance(&uri)?;

    let forwarded = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
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
        })
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        // No matching CSRs — update locally
        let not_updated = service.update_attrs(&id, fragment, &context).await.with_entity_instance(&uri)?;
        if not_updated.is_empty() {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        let not_updated_details: Vec<serde_json::Value> = not_updated
            .iter()
            .map(|attr| serde_json::json!({ "attributeName": attr, "reason": "Attribute not found" }))
            .collect();
        let body = serde_json::json!({ "updated": [], "notUpdated": not_updated_details });
        return Ok((StatusCode::MULTI_STATUS, Json(body)).into_response());
    }

    // Clause 5.6.2.4: the attributes proxied registrations cover leave the
    // fragment; whatever remains is updated here.
    let proxied = collect_proxied_attrs(&forwarded);
    let mut local_fragment = fragment;
    strip_attributes_from_value(&mut local_fragment, &proxied);
    let local = if local_fragment.as_object().is_some_and(|object| object.is_empty()) {
        LocalOutcome::NothingRemained
    } else {
        LocalOutcome::Processed(service.update_attrs(&id, local_fragment, &context).await.map(|_| ()))
    };
    provision_response(&id, &forwarded, local, StatusCode::NO_CONTENT.into_response(), &uri)
}
