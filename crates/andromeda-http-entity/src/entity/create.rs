//! HTTP handling for the create entity operation.

use crate::{
    entity::{
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
    },
    written_payload_admission::admit_written_entity,
};
use andromeda_entity::write_kind::CreateEntity;
use andromeda_federation::{forwarded_content::ForwardedContent, request::TriggeringRequest};
use andromeda_http_extraction::context_target;
use andromeda_http_response::headers::with_location;
use andromeda_jsonld::user_context_admission::admit_user_context;
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use axum::http::{HeaderMap, Method};

pub(crate) async fn create(
    context_target::ProvisionableTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantStateOrCreate,
    OriginalUri(uri): OriginalUri,
    ValidatedQuery(params): ValidatedQuery<LocalOnlyParams>,
    IncomingVia(incoming_via): IncomingVia,
    headers: HeaderMap,
    JsonLd { body, context }: JsonLd<Entity>,
) -> Result<Response, ProblemResponse> {
    // Clause 5.5.4 judges the payload "according to the @context", so the
    // context is admitted before anything is read under it (clause 5.5.7).
    let context = admit_user_context(state.entity_query.engine(), context).await.with_entity_instance(&uri)?;
    let entity = body.admitted(&context)?;
    let service = &state.entities;
    // Clause 5.6.1.4 opens with the clause 5.5.4 validation and forwards only
    // afterwards, so the whole screening runs here: a representation this
    // broker refuses reaches no context source, and a create whose attributes
    // a proxied registration claims in full is still validated rather than
    // answered 201 unexamined. Half of it is decidable only on the compact
    // payload -- after expansion a seal forged outside `ngsildproof` is
    // indistinguishable from a sub-attribute (clauses 4.5.2.2, 4.5.3.2 and
    // 4.5.3.3).
    let admitted = admit_written_entity::<CreateEntity>(service, &entity, &context)
        .await
        .with_entity_instance(&uri)?;
    let is_local = params.local.unwrap_or(false);

    if is_local {
        let id = service.create(&target, &entity.id, admitted).await.with_entity_instance(&uri)?;
        return Ok(with_location(StatusCode::CREATED.into_response(), format!("/ngsi-ld/v1/entities/{id}")));
    }

    // Check for matching CSRs for distributed forwarding
    let entity_id = entity.id.clone(); // clone: the forwarding request and local create both need the identifier
    let entity_type = match &entity.type_ {
        EntityTypeValue::Single(name) => Some(name),
        EntityTypeValue::Multiple(names) => names.first(),
    }
    .map(|name| TypeExprRaw::Type(name.clone())); // clone: the one-type selector owns the type it names

    let forwarded = state
        .forwarding
        .forward_provision(
            &target,
            ForwardProvisionRequest {
                resource: ProvisionResource::Entities,
                entity_id: &entity_id,
                entity_type_selector: entity_type.as_ref(),
                attribute_name: None,
                method: Method::POST,
                context: &context,
                operation: ForwardingOperation::CreateEntity,
                path_suffix: None,
                query_string: None,
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
        let id = service.create(&target, &entity_id, admitted).await.with_entity_instance(&uri)?;
        return Ok(with_location(StatusCode::CREATED.into_response(), format!("/ngsi-ld/v1/entities/{id}")));
    }

    // Clause 5.6.1.4: the attributes proxied registrations cover leave the
    // fragment; whatever remains creates the entity here.
    let local = match admitted.local_remainder(&collect_proxied_attrs(&forwarded)) {
        Some(remainder) => LocalOutcome::processed(service.create(&target, &entity_id, remainder).await, classify_entity_error),
        None => LocalOutcome::NothingRemained,
    };
    let created = with_location(StatusCode::CREATED.into_response(), format!("/ngsi-ld/v1/entities/{entity_id}"));
    provision_response(&entity_id, &forwarded, &local, created, &uri)
}
