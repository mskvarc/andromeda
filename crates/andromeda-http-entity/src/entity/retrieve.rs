//! HTTP handling for the retrieve entity operation.

use crate::{
    entity::{
        AcceptFormat,
        ContextSpec,
        Entity,
        EntityResultExt,
        EntityTenantState,
        GetEntityParams,
        HttpUri,
        IntoResponse,
        JoinRequest,
        NgsiLdCollectionResponse,
        NgsiLdErrorType,
        NgsiLdProtocolHeaders,
        NgsiLdRequestEnvelope,
        NgsiLdResponse,
        ProblemResponse,
        ResolvedJoin,
        Response,
        ResponsePayload,
        RetrievalNarrowing,
        RetrievingJoin,
        TypedPath,
        Uri,
        ValidatedQuery,
        Value,
        public_error,
        request_entity_map::{LocalEntitySource, record_map_use, resolve_retrieval_map_use},
        retrieve_forwarding::{RetrieveForwarding, registered_identity},
        start_rendering,
        with_linked_resolution_warnings,
    },
    read_rendering::{EntityRepresentation, render_entity, render_joined_entities},
};
use andromeda_federation::request::{EntityMapInUse, TriggeringRequest};
use andromeda_http_extraction::context_target;
use andromeda_http_headers::ngsild_entity_map::NgsiLdEntityMap;
use std::slice::from_ref;

/// What a retrieval writes into the response body.
enum RetrievedPayload {
    /// The entity itself, with an inline join's Linked Entities nested into it.
    Single(Value),
    /// The entity beside the Linked Entities a flat join reached, which is a
    /// collection even though one entity was asked for (clause 4.5.23).
    Collection(Vec<Value>),
}

/// The body a retrieval that resolved a clause 4.5.23 join answers with.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when an entity cannot be written in the
/// requested representation.
fn joined_payload(
    entity: &Entity,
    resolved: &ResolvedJoin<'_>,
    representation: &EntityRepresentation<'_>,
    instance: &HttpUri,
) -> Result<RetrievedPayload, ProblemResponse> {
    match resolved.join {
        // Flat: the root and its linked entities side by side.
        RetrievingJoin::Flat => {
            let entities = render_joined_entities(from_ref(entity), resolved, representation, instance)?;
            Ok(RetrievedPayload::Collection(entities))
        }
        // Inline: the renderer nests the linked entities into the root,
        // narrowing each nested copy by the sub-projection its own
        // Relationship carried (clause 4.21).
        RetrievingJoin::Inline => Ok(RetrievedPayload::Single(render_entity(entity, Some(resolved), representation, instance)?)),
    }
}

pub(crate) async fn get(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    accept: AcceptFormat,
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via,
        headers,
    }: NgsiLdRequestEnvelope,
    TypedPath(id): TypedPath<Uri>,
    ValidatedQuery(mut params): ValidatedQuery<GetEntityParams>,
    protocol: NgsiLdProtocolHeaders,
) -> Result<Response, ProblemResponse> {
    let service = &state.entity_query;
    let context: ContextSpec = context.into_inner();
    protocol.extensions().apply_to_entity_get(&mut params).with_entity_instance(&uri)?;
    let language_filter = params.lang.take().or(protocol.language_filter);
    let prefer = protocol.prefer;
    let entity_map_header = protocol.entity_map;

    // Validate the representation parameters before entity lookup so an invalid
    // `format` or `options` returns 400, not 404 (clause 5.5.4 general
    // NGSI-LD request-parameter validation).
    params.validate().with_entity_instance(&uri)?;

    let sys_attrs = params.options.unwrap_or_default().sys_attrs;
    // Clause 4.21 governs the single-entity retrieval exactly as it governs
    // a query: the compiled projection narrows the retrieved entity and, when
    // its terms carry clause 4.21 sub-projections, the entities the join
    // reaches from it.
    let projection = service
        .compile_projection(params.pick.as_deref(), params.omit.as_deref(), &context)
        .await
        .with_entity_instance(&uri)?;
    let narrowing = RetrievalNarrowing {
        attrs: params.attrs.as_deref().unwrap_or_default(),
        sys_attrs,
        dataset_ids: params.dataset_id.as_deref().unwrap_or_default(),
        projection: projection.as_ref(),
    };
    let local_result = service.get_inner(&target, &id, &narrowing, &context).await;

    let is_local = !target.permits_federation() || params.local.unwrap_or(false);
    let referenced = entity_map_header.as_ref().map(NgsiLdEntityMap::as_inner);
    let map_use = resolve_retrieval_map_use(&state.entity_maps, &target, &params, referenced, &uri).await?;

    // If local=true, skip distributed forwarding
    let (entity, warnings, entity_map_uri) = if is_local {
        (local_result.with_entity_instance(&uri)?, Vec::new(), None)
    } else {
        let local_entity = local_result.ok();
        let held_locally = local_entity.is_some();
        let forwarding = RetrieveForwarding {
            forwarding: &state.forwarding,
            target: &target,
            uri: &uri,
            entity_id: &id,
            params: &params,
            context: &context,
            triggering: TriggeringRequest {
                via: incoming_via.as_ref(),
                headers: Some(&headers),
            },
            entity_map: EntityMapInUse::from(&map_use),
        };
        let retrieved = forwarding.run(local_entity).await?;
        let entity = match retrieved.entity {
            Some(entity) => entity,
            // Clause 5.7.1.4: ResourceNotFound only when nothing is held
            // locally and no registration matches. A matching registration
            // that returned nothing leaves the entity known by its registered
            // identity alone, so the response is the entity's `id` and `type`.
            None => match registered_identity(&id, &retrieved.sources) {
                Some(entity) => entity,
                None => {
                    return Err(public_error(NgsiLdErrorType::ResourceNotFound, format!("Entity {id} not found"), &uri).with_warnings(retrieved.warnings));
                }
            },
        };
        // Table 6.5.3.1-1 `entityMap`: the map the retrieval used is returned,
        // created now when none was in use (clause 5.7.1.4), naming this
        // broker and every registration that returned the entity.
        let map_entities = LocalEntitySource::Retrieved(held_locally.then_some(&id));
        let map_uri = record_map_use(&state.entity_maps, map_use, retrieved.contributions, map_entities, &uri).await?;
        (entity, retrieved.warnings, map_uri)
    };

    let representation = EntityRepresentation::new(language_filter.as_ref(), params.resolve_render_mode(), projection.as_ref());
    let request = JoinRequest {
        join: params.join,
        join_level: params.join_level,
        contained_by: params.contained_by.as_deref(),
        sys_attrs,
        projection: projection.as_ref(),
    };
    let resolved = ResolvedJoin::prepare(service, &target, from_ref(&entity), &context, &request)
        .await
        .with_entity_instance(&uri)?;
    // Clause 6.3.17: a Linked Entity the join could not reach leaves the body
    // incomplete, and the warning header is what says so.
    let warnings = match &resolved {
        Some(resolved) => with_linked_resolution_warnings(warnings, &resolved.failures),
        None => warnings,
    };
    let payload = match &resolved {
        Some(resolved) => joined_payload(&entity, resolved, &representation, &uri)?,
        None => RetrievedPayload::Single(render_entity(&entity, None, &representation, &uri)?),
    };

    let rendering = start_rendering(accept.media_type(), &context, prefer.rendering_preferences()).with_geometry_property(params.geometry_property.as_ref());
    match payload {
        RetrievedPayload::Single(value) => {
            let rendered = rendering.with_payload(ResponsePayload::Single(value)).render().with_entity_instance(&uri)?;
            let mut response = NgsiLdResponse::ok(rendered).with_warnings(warnings);
            if let Some(map_uri) = entity_map_uri {
                response = response.with_entity_map_uri(map_uri);
            }
            Ok(response.into_response())
        }
        RetrievedPayload::Collection(values) => {
            let rendered = rendering
                .with_payload(ResponsePayload::Collection(values))
                .render()
                .with_entity_instance(&uri)?;
            let mut response = NgsiLdCollectionResponse::ok(rendered).with_warnings(warnings);
            if let Some(map_uri) = entity_map_uri {
                response = response.with_entity_map_uri(map_uri);
            }
            Ok(response.into_response())
        }
    }
}
