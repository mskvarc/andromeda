//! HTTP handling for the delete entity operation.

use crate::entity::{
    ContextSpec,
    ContextUrl,
    EntityResultExt,
    EntityTenantState,
    ForwardProvisionRequest,
    ForwardingOperation,
    IncomingVia,
    IntoResponse,
    LocalOnlyParams,
    LocalOutcome,
    OriginalUri,
    ProblemResponse,
    Response,
    StatusCode,
    TypedPath,
    Uri,
    ValidatedQuery,
    provision_response,
};
use andromeda_entity::error::EntityError;
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::tenant_state;
use andromeda_ngsi_ld_model::registration::ForwardingMode;
use axum::http::{HeaderMap, Method};

pub(crate) async fn delete(
    tenant_state::ExistingTenantState(state): EntityTenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath(id): TypedPath<Uri>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;
    let is_local = params.local.unwrap_or(false);

    if is_local {
        service.delete(&id).await.with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let context: ContextSpec = context.into_inner();

    let forwarded = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            entity_id: &id,
            entity_type_compact: None,
            attribute_name: None,
            method: Method::DELETE,
            body: None,
            context: &context,
            operation: ForwardingOperation::DeleteEntity,
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
        service.delete(&id).await.with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // Clause 5.6.6.4: proxied registrations delete their part; the entity this
    // broker holds for the unregistered attributes goes as well, and its
    // absence is no failure when the entity lives only at the sources.
    let proxied = forwarded
        .iter()
        .any(|result| matches!(result.mode, ForwardingMode::Exclusive | ForwardingMode::Redirect));
    let local = match service.delete(&id).await {
        Err(EntityError::NotFound { .. }) if proxied => LocalOutcome::NothingRemained,
        other => LocalOutcome::Processed(other.map(|_| ())),
    };
    provision_response(&id, &forwarded, local, StatusCode::NO_CONTENT.into_response(), &uri)
}
