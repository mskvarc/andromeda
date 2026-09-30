//! HTTP handling for the retrieve entity operation.

use crate::{
    entity::{
        AcceptFormat,
        Entity,
        EntityResultExt,
        EntityTenantState,
        GeometrySelection,
        GetEntityParams,
        HttpUri,
        Identifier,
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
        ResponseContextSource,
        ResponsePayload,
        RetrievalNarrowing,
        RetrievingJoin,
        TypedPath,
        ValidatedQuery,
        Value,
        public_error,
        request_entity_map::{LocalEntitySource, record_map_use, resolve_retrieval_map_use},
        retrieve_forwarding::RetrieveForwarding,
        with_linked_resolution_warnings,
    },
    read_rendering::{EntityRepresentation, apply_representation_names, render_entity, render_joined_entities},
};
use andromeda_entity::error::EntityError;
use andromeda_entity_map::recording::PageMatches;
use andromeda_federation::request::{EntityMapInUse, TriggeringRequest};
use andromeda_http_extraction::{context_target, response_context_request::ResponseContextRequest};
use andromeda_http_headers::{ngsild_entity_map::NgsiLdEntityMap, ngsild_warning::NgsiLdWarning};
use andromeda_ngsi_ld_render::response::ResponseRenderer;
use std::slice::{from_mut, from_ref};

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

/// The HTTP response a retrieval's rendered body becomes.
///
/// A clause 4.5.23 flat join answers a collection where every other retrieval
/// answers a single Entity, and each carries the Table 6.5.3.1-1 `entityMap`
/// header and the clause 6.3.17 warnings the read gathered. Which of the two
/// shapes is written is the only thing that differs, so the choice is made once
/// here rather than inside the handler that gathered them.
fn retrieval_response(
    payload: RetrievedPayload,
    rendering: ResponseRenderer<'_>,
    warnings: Vec<NgsiLdWarning>,
    entity_map_uri: Option<Identifier>,
    uri: &HttpUri,
) -> Result<Response, ProblemResponse> {
    match payload {
        RetrievedPayload::Single(value) => {
            let rendered = rendering.with_payload(ResponsePayload::Single(value)).render().with_entity_instance(uri)?;
            let mut response = NgsiLdResponse::ok(rendered).with_warnings(warnings);
            if let Some(map_uri) = entity_map_uri {
                response = response.with_entity_map_uri(map_uri);
            }
            Ok(response.into_response())
        }
        RetrievedPayload::Collection(values) => {
            let rendered = rendering.with_payload(ResponsePayload::Collection(values)).render().with_entity_instance(uri)?;
            let mut response = NgsiLdCollectionResponse::ok(rendered).with_warnings(warnings);
            if let Some(map_uri) = entity_map_uri {
                response = response.with_entity_map_uri(map_uri);
            }
            Ok(response.into_response())
        }
    }
}

/// The local entity to fold into a federated retrieval, or `None` when the local
/// store genuinely holds no such entity so the matching registrations answer.
///
/// A local failure that is not a genuine not-found -- a saturated pool
/// ([`EntityError::StoreSaturated`]), a persistence fault -- is not evidence the
/// entity is held nowhere, so it propagates unchanged to be classified on its
/// own terms (a saturated read answers 503 with `Retry-After`) rather than
/// masked as the `ResourceNotFound` an empty forwarding result would produce.
fn local_entity_for_forwarding(result: Result<Entity, EntityError>) -> Result<Option<Entity>, EntityError> {
    match result {
        Ok(entity) => Ok(Some(entity)),
        Err(EntityError::NotFound { .. }) => Ok(None),
        Err(other) => Err(other),
    }
}

pub(crate) async fn get(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    accept: AcceptFormat,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    ResponseContextRequest { context, api_root }: ResponseContextRequest,
    TypedPath(id): TypedPath<Identifier>,
    ValidatedQuery(mut params): ValidatedQuery<GetEntityParams>,
    protocol: NgsiLdProtocolHeaders,
) -> Result<Response, ProblemResponse> {
    let service = &state.entity_query;
    // Clause 5.5.7: a request `@context` declaring a JSON-LD Scoped Context is
    // refused on the read path exactly as on the write path, before anything is
    // expanded against it.
    service.admit_read_context(&context).await.with_entity_instance(&uri)?;
    protocol.extensions().apply_to_entity_get(&mut params).with_entity_instance(&uri)?;
    let language_filter = params.lang.take().or(protocol.language_filter);
    let prefer = protocol.prefer;
    let entity_map_header = protocol.entity_map;

    // Validate the representation parameters before entity lookup so an invalid
    // `format` or `options` returns 400, not 404 (clause 5.5.4 general
    // NGSI-LD request-parameter validation).
    params.validate().with_entity_instance(&uri)?;

    let sys_attrs = params.sys_attrs().with_entity_instance(&uri)?;
    // Clause 4.21 governs the single-entity retrieval exactly as it governs
    // a query: the compiled projection narrows the retrieved entity and, when
    // its terms carry clause 4.21 sub-projections, the entities the join
    // reaches from it.
    let projection = service
        .compile_projection(params.pick.as_deref(), params.omit.as_deref(), &context)
        .await
        .with_entity_instance(&uri)?;
    let narrowing = RetrievalNarrowing {
        entity_type: params.entity_type.as_ref(),
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
    let (mut entity, warnings, entity_map_uri) = if is_local {
        (local_result.with_entity_instance(&uri)?, Vec::new(), None)
    } else {
        let local_entity = local_entity_for_forwarding(local_result).with_entity_instance(&uri)?;
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
        // Clause 5.7.1.4: ResourceNotFound when the target Entity is held
        // neither locally nor by any source a matching registration forwarded
        // to. A registration that matched but whose sources returned no
        // Attribute data leaves nothing to answer with -- the broker does not
        // invent an Entity from the registration's id and type, and clause
        // 6.3.17 does not treat a source's empty answer as abnormal.
        let Some(entity) = retrieved.entity else {
            return Err(public_error(NgsiLdErrorType::ResourceNotFound, format!("entity {id} not found"), &uri).with_warnings(retrieved.warnings));
        };
        // Table 6.5.3.1-1 `entityMap`: the map the retrieval used is returned,
        // created now when none was in use (clause 5.7.1.4), naming this
        // broker and every registration that returned the entity.
        let map_entities = LocalEntitySource::Retrieved(held_locally.then_some(&id));
        // A retrieval applies no filters (clause 4.3.6.7), so it sheds nothing
        // and the match set is never consulted; the per-source reading is the
        // only one it could need.
        let map_uri = record_map_use(&state.entity_maps, map_use, retrieved.contributions, map_entities, PageMatches::PerSource, &uri).await?;
        (entity, retrieved.warnings, map_uri)
    };

    let representation = EntityRepresentation::new(
        language_filter.as_ref(),
        params.resolve_render_mode().with_entity_instance(&uri)?,
        projection.as_ref(),
    );
    let request = JoinRequest {
        join: params.join,
        join_level: params.join_level,
        contained_by: params.contained_by.as_deref(),
        sys_attrs,
        projection: projection.as_ref(),
    };
    let mut resolved = ResolvedJoin::prepare(service, &target, from_ref(&entity), &context, &request)
        .await
        .with_entity_instance(&uri)?;
    // Clause 6.3.17: a Linked Entity the join could not reach leaves the body
    // incomplete, and the warning header is what says so.
    let warnings = match &resolved {
        Some(resolved) => with_linked_resolution_warnings(warnings, &resolved.failures),
        None => warnings,
    };
    apply_representation_names(service, &representation, &context, from_mut(&mut entity), resolved.as_mut(), &uri).await?;
    let entity = entity;

    let payload = match &resolved {
        Some(resolved) => joined_payload(&entity, resolved, &representation, &uri)?,
        None => RetrievedPayload::Single(render_entity(&entity, None, &representation, &uri)?),
    };

    let rendering = ResponseContextSource::new(service.engine(), &context, &state.context_hosting, &api_root)
        .start_rendering(accept.media_type(), prefer.rendering_preferences(), representation.mode)
        .await
        .with_entity_instance(&uri)?
        .with_geometry_selection(GeometrySelection::new(
            params.geometry_property.as_ref(),
            params.dataset_id.as_deref().unwrap_or_default(),
        ));
    retrieval_response(payload, rendering, warnings, entity_map_uri, &uri)
}

#[cfg(test)]
mod tests {
    use crate::{entity::retrieve::local_entity_for_forwarding, entity_error::classify_entity_error};
    use andromeda_entity::error::EntityError;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_problem_details::problem_detail::ProblemClassification;

    #[test]
    fn a_saturated_local_read_is_a_503_not_a_404() {
        // A saturated local read propagates as StoreSaturated rather than being
        // swallowed into the not-found a federated retrieval would answer 404.
        let saturated = local_entity_for_forwarding(Err(EntityError::StoreSaturated));
        assert!(
            matches!(saturated, Err(EntityError::StoreSaturated)),
            "a saturated local read must propagate, not be masked as not-found"
        );
        assert_eq!(
            classify_entity_error(&EntityError::StoreSaturated).classification(),
            ProblemClassification::ServiceUnavailable,
            "a saturated read is a 503, never a 404"
        );

        // A genuine not-found yields no local entity, so the registrations answer.
        let identifier = "urn:ngsi-ld:Entity:absent".parse::<Identifier>().expect("valid test URI");
        let missing = local_entity_for_forwarding(Err(EntityError::NotFound { identifier }));
        assert!(matches!(missing, Ok(None)), "a genuine not-found forwards to the registrations");
    }
}
