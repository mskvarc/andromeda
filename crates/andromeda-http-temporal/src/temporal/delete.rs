//! HTTP handling for temporal delete (clause 5.6.16).

use crate::temporal::{
    ContextSpec,
    ContextUrl,
    ForwardProvisionRequest,
    ForwardingOperation,
    HeaderMap,
    IncomingVia,
    IntoResponse,
    LocalOnlyParams,
    LocalOutcome,
    Method,
    OriginalUri,
    ProblemResponse,
    ProvisionResource,
    Response,
    ResultInstanceExt,
    StatusCode,
    TemporalError,
    TenantState,
    TriggeringRequest,
    TypedPath,
    Uri,
    ValidatedQuery,
    classify_temporal_error,
    provision_response,
};
use andromeda_http_extraction::tenant_state;
use andromeda_ngsi_ld_model::registration::ForwardingMode;

pub async fn delete(
    tenant_state::ExistingTenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    context: ContextUrl,
    TypedPath(id): TypedPath<Uri>,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;

    if params.local.unwrap_or(false) {
        service.delete(&id).await.with_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let context: ContextSpec = context.into_inner();
    let forwarded = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            resource: ProvisionResource::TemporalEntities,
            entity_id: &id,
            entity_type_compact: None,
            attribute_name: None,
            method: Method::DELETE,
            body: None,
            context: &context,
            operation: ForwardingOperation::DeleteTemporalEntity,
            path_suffix: None,
            query_string: None,
            triggering: TriggeringRequest {
                via: incoming_via.as_ref(),
                headers: Some(&headers),
            },
        })
        .await
        .with_instance(&uri)?;

    // Clause 5.6.16.4: proxied registrations delete their part and need no
    // further processing; the evolution this broker holds goes as well, and
    // its absence is no failure when the evolution lives only at the sources.
    let proxied = forwarded
        .iter()
        .any(|result| matches!(result.mode, ForwardingMode::Exclusive | ForwardingMode::Redirect));
    let local = match service.delete(&id).await {
        Err(TemporalError::NotFound { .. }) if proxied => LocalOutcome::NothingRemained,
        other => LocalOutcome::processed(other, classify_temporal_error),
    };
    provision_response(&id, &forwarded, local, StatusCode::NO_CONTENT.into_response(), &uri)
}
