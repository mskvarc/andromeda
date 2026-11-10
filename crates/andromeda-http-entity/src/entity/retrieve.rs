//! HTTP handling for the retrieve entity operation.

use crate::{
    entity::{
        AcceptFormat,
        EntityResultExt,
        EntityTenantState,
        GeometrySelection,
        GetEntityParams,
        HttpUri,
        Identifier,
        IntoResponse,
        JoinRequest,
        NgsiLdCollectionResponse,
        NgsiLdProtocolHeaders,
        NgsiLdRequestEnvelope,
        NgsiLdResponse,
        ProblemResponse,
        ResolvedJoin,
        Response,
        ResponseContextSource,
        RetrievalNarrowing,
        TypedPath,
        ValidatedQuery,
        request_entity_map::{LocalEntitySource, record_map_use, resolve_retrieval_map_use},
        retrieve_forwarding::RetrieveForwarding,
        with_linked_resolution_warnings,
    },
    error::entity_not_found,
    read_rendering::{EntityRepresentation, RetrievedAnswer, retrieved_entity, settle_representation},
};
use andromeda_entity::{error::EntityError, query::entity_versions::VersionRead};
use andromeda_entity_map::recording::PageMatches;
use andromeda_federation::request::{EntityMapInUse, TriggeringRequest};
use andromeda_http_extraction::{context_target, path_segment::EntityId, response_context_request::ResponseContextRequest};
use andromeda_http_headers::{language::LanguageFilter, ngsild_entity_map::NgsiLdEntityMap, ngsild_warning::NgsiLdWarning};
use andromeda_jsonld::user_context_admission::{SubmittedContext, admit_user_context};
use andromeda_linked_retrieval::narrowing::list::ListInForce;
use andromeda_ngsi_ld_render::response::ResponseRenderer;
use andromeda_ngsi_ld_request::geometry_media_type::require_geojson_for_geometry_property;
use std::slice::{from_mut, from_ref};

/// The HTTP response a retrieval's answer becomes.
///
/// A clause 4.5.23 flat join answers a collection where every other retrieval
/// answers a single Entity, and each carries the Table 6.5.3.1-1 `entityMap`
/// header and the clause 6.3.17 warnings the read gathered. Which of the two
/// shapes is written is the only thing that differs, so the choice is made once
/// here rather than inside the handler that gathered them.
fn retrieval_response(
    answer: RetrievedAnswer<'_>,
    rendering: ResponseRenderer<'_>,
    language_filter: Option<&LanguageFilter>,
    warnings: Vec<NgsiLdWarning>,
    entity_map_uri: Option<Identifier>,
    uri: &HttpUri,
) -> Result<Response, ProblemResponse> {
    match answer {
        RetrievedAnswer::Single(answer) => {
            let rendered = answer.render(rendering, language_filter, uri)?;
            let mut response = NgsiLdResponse::ok(rendered).with_warnings(warnings);
            if let Some(map_uri) = entity_map_uri {
                response = response.with_entity_map_uri(map_uri);
            }
            Ok(response.into_response())
        }
        RetrievedAnswer::Collection(answer) => {
            let rendered = answer.render(rendering, language_filter, uri)?;
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
/// A local failure that is not a genuine not-found (a saturated pool
/// ([`EntityError::StoreSaturated`]), a persistence fault) is not evidence the
/// entity is held nowhere, so it propagates unchanged to be classified on its
/// own terms (a saturated read answers 503 with `Retry-After`) rather than
/// masked as the `ResourceNotFound` an empty forwarding result would produce.
fn local_entity_for_forwarding<T>(result: Result<T, EntityError>) -> Result<Option<T>, EntityError> {
    match result {
        Ok(entity) => Ok(Some(entity)),
        Err(EntityError::NotFound { .. }) => Ok(None),
        Err(other) => Err(other),
    }
}

/// Retrieve Entity (clause 5.7.1); a clause 4.5.23 flat join answers a collection instead.
pub(crate) async fn get(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    accept: AcceptFormat,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    ResponseContextRequest { context, api_root }: ResponseContextRequest,
    TypedPath(EntityId(id)): TypedPath<EntityId>,
    ValidatedQuery(mut params): ValidatedQuery<GetEntityParams>,
    protocol: NgsiLdProtocolHeaders,
) -> Result<Response, ProblemResponse> {
    let service = &state.entity_query;
    // Clause 5.5.7: the request `@context` is admitted on the read path exactly
    // as on the write path, before anything is expanded against it.
    let context = admit_user_context(service.engine(), SubmittedContext::from(context))
        .await
        .with_entity_instance(&uri)?;
    let language_filter = params.lang.take().or(protocol.language_filter);
    let prefer = protocol.prefer;
    let entity_map_header = protocol.entity_map;

    // Validated before the entity lookup so an invalid `format` or `options`
    // answers 400, not 404 (clause 5.5.4).
    params.validate().with_entity_instance(&uri)?;
    require_geojson_for_geometry_property(params.geometry_property.as_ref(), accept.media_type()).with_entity_instance(&uri)?;

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
    let local_result = service.read_held(&target, &id, &narrowing, &context).await;

    let is_local = !target.permits_federation() || params.local.unwrap_or(false);
    let referenced = entity_map_header.as_ref().map(NgsiLdEntityMap::as_inner);
    let map_use = resolve_retrieval_map_use(&state.entity_maps, &target, &params, referenced, &uri).await?;

    let (mut held, warnings, entity_map_uri) = if is_local {
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
        // Attribute data leaves nothing to answer with: the broker does not
        // invent an Entity from the registration's id and type, and clause
        // 6.3.17 does not treat a source's empty answer as abnormal.
        let Some(merged) = retrieved.entity else {
            return Err(entity_not_found(&id, &uri).with_warnings(retrieved.warnings));
        };
        // Table 6.5.3.1-1 `entityMap`: the map the retrieval used is returned,
        // created now when none was in use (clause 5.7.1.4), naming this
        // broker and every registration that returned the entity.
        let map_entities = LocalEntitySource::Retrieved(held_locally.then_some(&id));
        // A retrieval applies no filters (clause 4.3.6.7), so it sheds nothing
        // and the match set is never consulted; the per-source reading is the
        // only one it could need.
        let map_uri = record_map_use(&state.entity_maps, map_use, retrieved.contributions, map_entities, PageMatches::PerSource, &uri).await?;
        (merged, retrieved.warnings, map_uri)
    };
    // The entity is compacted once, after the merge, under the request
    // context: what every holder contributed is written the same way.
    let read = VersionRead {
        id_override: Some(&id),
        sys_attrs,
        dataset_ids: params.dataset_id.as_deref().unwrap_or_default(),
    };
    let carried = ResolvedJoin::carried_by(from_mut(&mut held));
    let mut entity = service
        .compact_versions(from_ref(&held), &read, &context)
        .await
        .with_entity_instance(&uri)?
        .pop()
        .ok_or_else(|| entity_not_found(&id, &uri))?;

    let representation = EntityRepresentation::new(params.resolve_render_mode().with_entity_instance(&uri)?, projection.as_ref());
    // Table 6.5.3.1-1 makes `attrs` the `pick` synonym, so it narrows every
    // Linked Entity as `pick` would (clause 5.7.1.5).
    let attrs = service
        .expand_attribute_names(params.attrs.as_deref().unwrap_or_default(), &context)
        .await
        .with_entity_instance(&uri)?;
    let request = JoinRequest {
        join: params.join,
        join_level: params.join_level,
        contained_by: params.contained_by.as_deref(),
        sys_attrs,
        list: ListInForce::of(projection.as_ref(), &attrs),
        carried: &carried,
    };
    let mut resolved = ResolvedJoin::prepare(service, &target, from_ref(&entity), &context, &request)
        .await
        .with_entity_instance(&uri)?;
    // Clause 6.3.17: a Linked Entity the join could not reach leaves the body
    // incomplete, and the warning header is what says so.
    let warnings = with_linked_resolution_warnings(warnings, ResolvedJoin::failures_of(resolved.as_ref()));
    settle_representation(service, &representation, &context, from_mut(&mut entity), resolved.as_mut(), &uri).await?;
    let entity = entity;
    let resolved = resolved;
    let plan = resolved.as_ref().map(ResolvedJoin::plan);

    let answer = retrieved_entity(&entity, plan.as_ref(), &representation);

    let rendering = ResponseContextSource::new(service.engine(), &context, &state.context_hosting, &api_root)
        .start_rendering(accept.media_type(), prefer.rendering_preferences(), representation.mode)
        .await
        .with_entity_instance(&uri)?
        .with_geometry_selection(GeometrySelection::new(
            params.geometry_property.as_ref(),
            params.dataset_id.as_deref().unwrap_or_default(),
        ));
    retrieval_response(answer, rendering, language_filter.as_ref(), warnings, entity_map_uri, &uri)
}

#[cfg(test)]
mod tests {
    use crate::entity::retrieve::local_entity_for_forwarding;
    use andromeda_entity::error::EntityError;
    use andromeda_failure_classification::entity_failure::classify_entity_error;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_problem_details::problem_detail::ProblemClassification;

    #[test]
    fn a_saturated_local_read_is_a_503_not_a_404() {
        // A saturated local read propagates as StoreSaturated rather than being
        // swallowed into the not-found a federated retrieval would answer 404.
        let saturated = local_entity_for_forwarding::<()>(Err(EntityError::StoreSaturated));
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
        let missing = local_entity_for_forwarding::<()>(Err(EntityError::NotFound { identifier }));
        assert!(matches!(missing, Ok(None)), "a genuine not-found forwards to the registrations");
    }
}
