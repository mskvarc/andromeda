//! HTTP handling for the retrieve entity operation.

use crate::entity::{
    AcceptFormat,
    ContextSpec,
    ContextUrl,
    CsrModeFilter,
    Entity,
    EntityRenderer,
    EntityResultExt,
    EntityTenantState,
    EntityTypeValue,
    ForwardingOperation,
    GetEntityParams,
    IntoResponse,
    JoinRequest,
    NgsiLdCollectionResponse,
    NgsiLdErrorType,
    NgsiLdProtocolHeaders,
    NgsiLdResponse,
    OriginalUri,
    ProblemResponse,
    ResolvedJoin,
    Response,
    ResponsePayload,
    RetrievalNarrowing,
    RetrievingJoin,
    SourceContribution,
    TypedPath,
    Uri,
    ValidatedQuery,
    collect_warnings,
    contributions_of,
    merge_retrieve_results,
    public_error,
    render_flat_join,
    single_source_declined,
    start_rendering,
    with_linked_resolution_warnings,
};
use andromeda_entity_map::{
    fingerprint::{FingerprintedRequest, fingerprint_of},
    reuse::{EntityMapFlag, EntityMapUse},
};
use andromeda_federation::{
    forwarded_query::ClientQuery,
    outcome::{ForwardReadOutcome, ForwardedEntities, MatchedSources},
    request::{EntityMapInUse, ForwardRetrieveRequest, TriggeringRequest},
};
use andromeda_http_extraction::context_target;
use andromeda_http_headers::{ngsild_entity_map::NgsiLdEntityMap, ngsild_warning::NgsiLdWarning};
use axum::http::HeaderMap;
use std::{iter::once, slice::from_ref};
use tracing::warn;

/// What the distributed part of a retrieval brought back (clause 5.7.1.4).
struct ForwardedRetrieval {
    /// The entity, merged from every source that held it.
    entity: Option<Entity>,
    /// The clause 6.3.17 warnings of both forwarding phases.
    warnings: Vec<NgsiLdWarning>,
    /// What each source contributed to the map of the request (clause 5.2.39).
    contributions: Vec<SourceContribution>,
    /// What the primary registration selection said.
    sources: MatchedSources,
}

pub(crate) async fn get(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    TypedPath(id): TypedPath<Uri>,
    ValidatedQuery(mut params): ValidatedQuery<GetEntityParams>,
    protocol: NgsiLdProtocolHeaders,
    headers: HeaderMap,
) -> Result<Response, ProblemResponse> {
    let service = &state.entity_query;
    let context: ContextSpec = context.into_inner();
    protocol.extensions().apply_to_entity_get(&mut params).with_entity_instance(&uri)?;
    let language_filter = params.lang.take().or(protocol.language_filter);
    let prefer = protocol.prefer;
    let incoming_via = protocol.incoming_via;
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

    // Clause 5.7.1.4: a referenced live EntityMap fixes the registrations the
    // retrieval contacts; an expired or unknown one yields no inference, so a
    // fresh map is created instead. A local retrieval contacts no source and
    // uses no map.
    let map_use = if is_local {
        EntityMapUse::None
    } else {
        let fingerprint = fingerprint_of(FingerprintedRequest::RetrieveEntity(&params)).with_entity_instance(&uri)?;
        state
            .entity_maps
            .resolve_use(
                entity_map_header.as_ref().map(NgsiLdEntityMap::as_inner),
                EntityMapFlag::from_parameter(params.entity_map),
                &fingerprint,
                params.entity_map_lifetime.as_ref(),
            )
            .await
            .with_entity_instance(&uri)?
    };
    let mut entity_map_uri = None;

    // If local=true, skip distributed forwarding
    let (entity, warnings) = if is_local {
        let entity = local_result.with_entity_instance(&uri)?;
        (entity, Vec::new())
    } else {
        let local_entity = local_result.ok();
        let held_locally = local_entity.is_some();
        let triggering = TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        };
        let phase = |mode_filter: CsrModeFilter| {
            let forwarding = &state.forwarding;
            let params = &params;
            let context = &context;
            let id = &id;
            let target = &target;
            let entity_map = EntityMapInUse::from(&map_use);
            async move {
                forwarding
                    .forward_retrieve(
                        target,
                        ForwardRetrieveRequest {
                            entity_id: id,
                            exclude_registration: None,
                            client_query: ClientQuery::EntityRetrieve(params),
                            context,
                            operation: ForwardingOperation::RetrieveEntity,
                            triggering,
                            mode_filter,
                            entity_map,
                        },
                    )
                    .await
                    .unwrap_or_else(|error| {
                        // Retrieve forwarding is best-effort: a source that
                        // cannot be reached is a warning, not a failure.
                        warn!(%error, "context source forwarding failed during the entity retrieval");
                        ForwardReadOutcome::empty()
                    })
            }
        };
        let retrieved = retrieve_distributed(local_entity, phase)
            .await
            .map_err(|status| single_source_declined(status, &uri))?;
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
        // broker and every registration that returned the entity; a reused
        // map records the maps its sources answered with (clause 5.5.14).
        entity_map_uri = match map_use {
            EntityMapUse::Reuse(map) => {
                let links = retrieved
                    .contributions
                    .iter()
                    .filter_map(|contribution| contribution.linked_map.as_ref().map(|linked| (&contribution.registration, linked)));
                state.entity_maps.record_linked_maps(&map.id, links).await.with_entity_instance(&uri)?;
                Some(map.id)
            }
            EntityMapUse::Mint(mut map) => {
                map.add_local_entities(held_locally.then(|| id.clone())); // clone: the map owns its entity identifiers
                for contribution in retrieved.contributions {
                    map.add_contribution(&contribution.registration, contribution.entity_ids, contribution.linked_map);
                }
                state.entity_maps.store(&map).await.with_entity_instance(&uri)?;
                Some(map.id)
            }
            EntityMapUse::None => None,
        };
        (entity, retrieved.warnings)
    };

    let mode = params.resolve_render_mode();

    // Join mode (clause 4.5.23) is parsed at the query boundary. `@none` and an
    // absent `join` retrieve no linked entity at all, so neither reaches the
    // traversal.
    if let Some(join) = RetrievingJoin::from_mode(params.join) {
        let request = JoinRequest {
            join,
            join_level: params.join_level,
            contained_by: params.contained_by.as_deref(),
            sys_attrs,
            projection: projection.as_ref(),
        };
        let resolved = ResolvedJoin::prepare(service, &target, from_ref(&entity), &context, &request)
            .await
            .with_entity_instance(&uri)?;
        // Clause 6.3.17: a Linked Entity the join could not reach leaves the
        // body incomplete, and the warning header is what says so.
        let warnings = with_linked_resolution_warnings(warnings, &resolved.failures);

        match join {
            RetrievingJoin::Flat => {
                // Flat: the root and its linked entities side by side (clause 4.5.23).
                let all_entities = render_flat_join(once(&entity), resolved.linked.values(), language_filter.as_ref(), mode, projection.as_ref())
                    .with_entity_instance(&uri)?;
                let rendered = start_rendering(accept.media_type(), &context, prefer.rendering_preferences())
                    .with_geometry_property(params.geometry_property.as_ref())
                    .with_payload(ResponsePayload::Collection(all_entities))
                    .render()
                    .with_entity_instance(&uri)?;
                let mut response = NgsiLdCollectionResponse::ok(rendered).with_warnings(warnings);
                if let Some(map_uri) = entity_map_uri {
                    response = response.with_entity_map_uri(map_uri);
                }
                return Ok(response.into_response());
            }
            RetrievingJoin::Inline => {
                // Inline: the renderer nests the linked entities into the
                // root, narrowing each nested copy by the sub-projection its
                // own Relationship carried (clause 4.21).
                let entity_value = EntityRenderer::new(&entity)
                    .with_optional_language_filter(language_filter.as_ref())
                    .with_inline_linked_entities(&resolved.linked)
                    .with_optional_linked_projection(resolved.linked_projection())
                    .with_optional_structural_projection(projection.as_ref())
                    .with_mode(mode)
                    .render()
                    .with_entity_instance(&uri)?;
                let rendered = start_rendering(accept.media_type(), &context, prefer.rendering_preferences())
                    .with_geometry_property(params.geometry_property.as_ref())
                    .with_payload(ResponsePayload::Single(entity_value))
                    .render()
                    .with_entity_instance(&uri)?;
                let mut response = NgsiLdResponse::ok(rendered).with_warnings(warnings);
                if let Some(map_uri) = entity_map_uri {
                    response = response.with_entity_map_uri(map_uri);
                }
                return Ok(response.into_response());
            }
        }
    }

    let entity_value = EntityRenderer::new(&entity)
        .with_optional_language_filter(language_filter.as_ref())
        .with_optional_structural_projection(projection.as_ref())
        .with_mode(mode)
        .render()
        .with_entity_instance(&uri)?;
    let rendered = start_rendering(accept.media_type(), &context, prefer.rendering_preferences())
        .with_geometry_property(params.geometry_property.as_ref())
        .with_payload(ResponsePayload::Single(entity_value))
        .render()
        .with_entity_instance(&uri)?;
    let mut response = NgsiLdResponse::ok(rendered).with_warnings(warnings);
    if let Some(map_uri) = entity_map_uri {
        response = response.with_entity_map_uri(map_uri);
    }
    Ok(response.into_response())
}

/// The two clause 5.7.1.4 forwarding phases over `phase`, merged onto the
/// local entity: the exclusive, redirect and inclusive sources first, then
/// the auxiliary ones, which only supplement an entity found elsewhere
/// (clause 4.3.6.2).
///
/// # Errors
///
/// The status of the one proxied source that declined the representation
/// (Table 6.4.3.2-3), which is the operation's answer.
async fn retrieve_distributed<F, Fut>(local_entity: Option<Entity>, phase: F) -> Result<ForwardedRetrieval, u16>
where
    F: Fn(CsrModeFilter) -> Fut,
    Fut: Future<Output = ForwardReadOutcome<ForwardedEntities>>,
{
    let primary = phase(CsrModeFilter::ExcludeAuxiliary).await;
    if let Some(status) = primary.sources.single_source_status {
        return Err(status);
    }
    let mut warnings = collect_warnings(&primary.results);
    let mut contributions: Vec<SourceContribution> = contributions_of(&primary.results).collect();
    let sources = primary.sources;
    let merged = merge_retrieve_results(local_entity, primary.results);

    // Phase 2: auxiliary sources only if an entity was found, since they
    // only supplement existing data (clause 4.3.6.2).
    let entity = match merged {
        Some(merged) => {
            let auxiliary = phase(CsrModeFilter::AuxiliaryOnly).await;
            warnings.extend(collect_warnings(&auxiliary.results));
            contributions.extend(contributions_of(&auxiliary.results));
            merge_retrieve_results(Some(merged), auxiliary.results)
        }
        None => None,
    };
    Ok(ForwardedRetrieval {
        entity,
        warnings,
        contributions,
        sources,
    })
}

/// The entity a retrieval answers with when a registration matched but no
/// source returned attribute data (clause 5.7.1.4): its `id` and the type the
/// registration registers it under.
fn registered_identity(id: &Uri, sources: &MatchedSources) -> Option<Entity> {
    if sources.registrations == 0 {
        return None;
    }
    let registered_type = sources.registered_type.clone()?; // clone: the synthesized entity owns its type
    Some(
        Entity::builder()
            .id(id.clone()) // clone: the synthesized entity owns its identifier
            .type_(EntityTypeValue::Single(registered_type))
            .build(),
    )
}
