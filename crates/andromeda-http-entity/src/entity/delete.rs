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
    ProvisionResource,
    Response,
    StatusCode,
    TypedPath,
    Uri,
    ValidatedQuery,
    classify_entity_error,
    provision_response,
};
use andromeda_entity::error::EntityError;
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::context_target;
use andromeda_ngsi_ld_model::registration::ForwardingMode;
use axum::http::{HeaderMap, Method};

pub(crate) async fn delete(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
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
        service.delete(&target, &id).await.with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let context: ContextSpec = context.into_inner();

    let forwarded = state
        .forwarding
        .forward_provision(
            &target,
            ForwardProvisionRequest {
                resource: ProvisionResource::Entities,
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
            },
        )
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        service.delete(&target, &id).await.with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // Clause 5.6.6.4: proxied registrations delete their part, and the local
    // row holding the unregistered attributes goes with it. Clause 6.3.17
    // treats the entity as held at the proxied source, so removing that row
    // is no part the client can be told succeeded on its own: a failure at the
    // source is the operation's failure, and the row's absence is no failure.
    let proxied = forwarded
        .iter()
        .any(|result| matches!(result.mode, ForwardingMode::Exclusive | ForwardingMode::Redirect));
    let local = match service.delete(&target, &id).await {
        Ok(()) | Err(EntityError::NotFound { .. }) if proxied => LocalOutcome::NothingRemained,
        other => LocalOutcome::processed(other, classify_entity_error),
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}
