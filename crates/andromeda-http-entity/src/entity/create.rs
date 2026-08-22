//! HTTP handling for the create entity operation.

use crate::entity::{
    ContextUrl,
    Entity,
    EntityResultExt,
    EntityTenantStateOrCreate,
    EntityTypeValue,
    ForwardProvisionRequest,
    ForwardingOperation,
    IncomingVia,
    IntoResponse,
    JsonLd,
    LocalOnlyParams,
    LocalOutcome,
    OriginalUri,
    ProblemResponse,
    ProvisionResource,
    Response,
    StatusCode,
    ValidatedQuery,
    classify_entity_error,
    collect_proxied_attrs,
    provision_response,
};
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::tenant_state;
use andromeda_http_response::headers::with_location;
use axum::http::{HeaderMap, Method};

pub(crate) async fn create(
    tenant_state::ProvisionableTenantState(state): EntityTenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    context: ContextUrl,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
    JsonLd {
        value: entity,
        context: body_context,
    }: JsonLd<Entity>,
) -> Result<Response, ProblemResponse> {
    let service = &state.entities;
    let context = context.merge(body_context);
    let is_local = params.local.unwrap_or(false);

    if is_local {
        let id = service.create(&entity, &context).await.with_entity_instance(&uri)?;
        return Ok(with_location(StatusCode::CREATED.into_response(), format!("/ngsi-ld/v1/entities/{}", id)));
    }

    // Check for matching CSRs for distributed forwarding
    let entity_body = serde_json::to_vec(&entity).with_entity_instance(&uri)?;
    let entity_id = entity.id.clone(); // clone: the forwarding request and local create both need the identifier
    let entity_type = match &entity.type_ {
        EntityTypeValue::Single(name) => Some(name),
        EntityTypeValue::Multiple(names) => names.first(),
    };

    let forwarded = state
        .forwarding
        .forward_provision(ForwardProvisionRequest {
            resource: ProvisionResource::Entities,
            entity_id: &entity_id,
            entity_type_compact: entity_type,
            attribute_name: None,
            method: Method::POST,
            body: Some(&entity_body),
            context: &context,
            operation: ForwardingOperation::CreateEntity,
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
        let id = service.create(&entity, &context).await.with_entity_instance(&uri)?;
        return Ok(with_location(StatusCode::CREATED.into_response(), format!("/ngsi-ld/v1/entities/{}", id)));
    }

    // Clause 5.6.1.4: the attributes proxied registrations cover leave the
    // fragment; whatever remains creates the entity here.
    let proxied = collect_proxied_attrs(&forwarded);
    let mut entity = entity;
    let local = if entity.strip_attributes(&proxied) {
        LocalOutcome::processed(service.create(&entity, &context).await, classify_entity_error)
    } else {
        LocalOutcome::NothingRemained
    };
    let created = with_location(StatusCode::CREATED.into_response(), format!("/ngsi-ld/v1/entities/{entity_id}"));
    provision_response(&entity_id, &forwarded, local, created, &uri)
}
