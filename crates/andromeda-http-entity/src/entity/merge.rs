//! HTTP handling for the merge entity operation.

use crate::entity::{
    ContextUrl,
    EntityPatch,
    EntityResultExt,
    EntityTenantState,
    ForwardProvisionRequest,
    ForwardingOperation,
    IncomingVia,
    IntoResponse,
    JsonLd,
    LocalOnlyParams,
    LocalOutcome,
    OriginalUri,
    ProblemResponse,
    Response,
    StatusCode,
    TypedPath,
    Uri,
    ValidatedQuery,
    collect_proxied_attrs,
    provision_response,
};
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::tenant_state;
use axum::http::{HeaderMap, Method};

pub(crate) async fn merge(
    tenant_state::ExistingTenantState(state): EntityTenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath(id): TypedPath<Uri>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
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
    let patch_body = serde_json::to_vec(&patch).with_entity_instance(&uri)?;

    let forwarded = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            entity_id: &id,
            entity_type_compact: None,
            attribute_name: None,
            method: Method::PATCH,
            body: Some(&patch_body),
            context: &context,
            operation: ForwardingOperation::MergeEntity,
            path_suffix: None,
            query_string: None,
            triggering: TriggeringRequest {
                via: incoming_via.as_ref(),
                headers: Some(&headers),
            },
        })
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        service.merge(&id, &patch, &context).await.with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // Clause 5.6.17.4: the attributes proxied registrations cover leave the
    // fragment; whatever remains is merged here.
    let proxied = collect_proxied_attrs(&forwarded);
    let mut patch = patch;
    let local = if patch.strip_attributes(&proxied) {
        LocalOutcome::Processed(service.merge(&id, &patch, &context).await.map(|_| ()))
    } else {
        LocalOutcome::NothingRemained
    };
    provision_response(&id, &forwarded, local, StatusCode::NO_CONTENT.into_response(), &uri)
}
