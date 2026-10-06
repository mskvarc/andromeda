//! HTTP handling for the delete entity operation.

use crate::{
    entity::{
        ContextSpec,
        ContextUrl,
        EntityResultExt,
        EntityTenantState,
        EntityWriteParams,
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
        provision_response,
    },
    forwarded_write_query::{forwarded_type_parameter, forwarded_write_query},
};
use andromeda_entity::{error::EntityError, write_target::WriteTarget};
use andromeda_federation::{outcome::ForwardedWriteOutcome, request::TriggeringRequest};
use andromeda_http_extraction::context_target;
use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
use axum::http::{HeaderMap, Method};

pub(crate) async fn delete(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath(id): TypedPath<Identifier>,
    ValidatedQuery(params): ValidatedQuery<EntityWriteParams>,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;
    let is_local = params.local.unwrap_or(false);
    let context: ContextSpec = context.into_inner();
    let selector = service
        .expand_write_selector(params.entity_type.as_ref(), &context)
        .await
        .with_entity_instance(&uri)?;
    let entity = WriteTarget::new(&id, selector.as_ref());

    if is_local {
        service.delete(&target, entity).await.with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let qs = forwarded_write_query([forwarded_type_parameter(uri.query())]);

    let forwarded = state
        .forwarding
        .forward_provision(
            &target,
            ForwardProvisionRequest {
                resource: ProvisionResource::Entities,
                entity_id: &id,
                entity_type_selector: params.entity_type.as_ref(),
                attribute_name: None,
                method: Method::DELETE,
                context: &context,
                operation: ForwardingOperation::DeleteEntity,
                path_suffix: None,
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
        service.delete(&target, entity).await.with_entity_instance(&uri)?;
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
    // Clause 5.6.6.4: when the matched proxied registrations do not advertise
    // Delete Entity, the delete of their part is unsupported and never sent -- a
    // Conflict "in case the complete delete Entity failed, or ... a partial success
    // if some parts of the delete Entity succeeded". A local row that was actually
    // removed is that succeeded part, so an unsupported-everywhere delete beside a
    // successful local removal is a 207 partial success, not a 409.
    let unsupported_only = forwarded.iter().all(|result| matches!(result.outcome, ForwardedWriteOutcome::NotAdvertised));
    let local = match service.delete(&target, entity).await {
        Ok(()) if proxied && unsupported_only => LocalOutcome::Processed(Ok(())),
        Ok(()) | Err(EntityError::NotFound { .. }) if proxied => LocalOutcome::NothingRemained,
        other => LocalOutcome::processed(other, classify_entity_error),
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}
