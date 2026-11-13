//! HTTP handling for the replace entity operation.

use crate::{
    entity::{
        EntityPatch,
        EntityResultExt,
        EntityTenantState,
        EntityWriteParams,
        ForwardProvisionRequest,
        ForwardingOperation,
        IntoResponse,
        JsonLd,
        LocalOutcome,
        NgsiLdRequestEnvelope,
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
    written_payload_admission::admit_written_entity,
};
use andromeda_entity::{write_kind::ReplaceEntity, write_target::WriteTarget};
use andromeda_federation::{forwarded_content::ForwardedContent, request::TriggeringRequest};
use andromeda_http_extraction::{context_target, path_segment::EntityId};
use andromeda_jsonld::user_context_admission::admit_user_context;
use andromeda_ngsi_ld_entity::admission::written_families::WrittenFamilies;
use axum::http::Method;

/// Replace Entity (clause 5.6.18): 204 when every part succeeded, 207 with the
/// clause 5.2.16 result when only some did.
pub(crate) async fn update(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    TypedPath(EntityId(id)): TypedPath<EntityId>,
    ValidatedQuery(params): ValidatedQuery<EntityWriteParams>,
    JsonLd { body, context }: JsonLd<EntityPatch>,
) -> Result<Response, ProblemResponse> {
    // Clause 5.5.4 judges the payload "according to the @context", so the
    // context is admitted before anything is read under it (clause 5.5.7).
    let context = admit_user_context(state.entity_query.engine(), context).await.with_entity_instance(&uri)?;
    let patch = body.admitted(&context).await?;
    let service = &state.entities;
    let is_local = params.local.unwrap_or(false);
    let selector = service
        .expand_write_selector(params.entity_type.as_ref(), &context)
        .await
        .with_entity_instance(&uri)?;
    let entity = WriteTarget::new(&id, selector.as_ref());
    let qs = forwarded_write_query([forwarded_type_parameter(uri.query())]);
    // Clause 5.6.18.4 runs the clause 5.5.4 validation before the forwarding
    // phase, so a payload this broker refuses reaches no context source and a
    // replacement left with nothing local is still validated.
    let admitted = admit_written_entity::<ReplaceEntity>(service, &patch, &context, WrittenFamilies::Final)
        .await
        .with_entity_instance(&uri)?;

    if is_local {
        service.replace_entity(&target, entity, admitted).await.with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let forwarded = state
        .forwarding
        .forward_provision(
            &target,
            ForwardProvisionRequest {
                resource: ProvisionResource::Entities,
                entity_id: &id,
                entity_type_selector: params.entity_type.as_ref(),
                attribute_name: None,
                method: Method::PUT,
                context: &context,
                operation: ForwardingOperation::ReplaceEntity,
                path_suffix: None,
                query_string: qs.as_deref(),
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
            },
            Some(ForwardedContent::Entity(admitted.document())),
        )
        .await
        .with_entity_instance(&uri)?;

    if forwarded.is_empty() {
        service.replace_entity(&target, entity, admitted).await.with_entity_instance(&uri)?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    // Clause 5.6.18.4: the attributes proxied registrations cover leave the
    // fragment; whatever remains replaces the entity here.
    let local = match admitted.local_remainder(&collect_proxied_attrs(&forwarded)) {
        Some(remainder) => local_entity_outcome(service.replace_entity(&target, entity, remainder).await, &forwarded),
        None => LocalOutcome::NothingRemained,
    };
    provision_response(&id, &forwarded, &local, StatusCode::NO_CONTENT.into_response(), &uri)
}
