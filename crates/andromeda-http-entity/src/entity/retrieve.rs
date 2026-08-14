//! HTTP handling for the retrieve entity operation.

use crate::entity::{
    AcceptFormat,
    ContextSpec,
    ContextUrl,
    CsrModeFilter,
    EntityRenderer,
    EntityResultExt,
    EntityTenantState,
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
    SnapshotRenderer,
    TypedPath,
    Uri,
    ValidatedQuery,
    collect_warnings,
    merge_retrieve_results,
    public_error,
    render_flat_join,
    start_rendering,
    with_linked_resolution_warnings,
};
use std::{iter::once, slice::from_ref};

pub(crate) async fn get(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): EntityTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    TypedPath(id): TypedPath<Uri>,
    ValidatedQuery(mut params): ValidatedQuery<GetEntityParams>,
    protocol: NgsiLdProtocolHeaders,
) -> Result<Response, ProblemResponse> {
    let service = &state.entity_query;
    let context: ContextSpec = context.into_inner();
    protocol.extensions().apply_to_entity_get(&mut params).with_entity_instance(&uri)?;
    let language_filter = params.lang.take().or(protocol.language_filter);
    let prefer = protocol.prefer;
    let snapshot = protocol.snapshot;
    let incoming_via = protocol.incoming_via;

    // Validate the representation parameters before entity lookup so an invalid
    // `format` or `options` returns 400, not 404 (clause 5.5.4 general
    // NGSI-LD request-parameter validation).
    params.validate().with_entity_instance(&uri)?;

    // Snapshot scoping (ETSI GS CIM 009 clause 5.5.15, 6.3.22): if NGSILD-Snapshot header
    // is present, retrieve entity from the snapshot's frozen data and return early.
    if let Some(snapshot_id) = snapshot.into_inner() {
        state.snapshots.validate(&snapshot_id).await.with_entity_instance(&uri)?;

        let entity_data = state
            .snapshots
            .entity(&snapshot_id, &id)
            .await
            .with_entity_instance(&uri)?
            .ok_or_else(|| public_error(NgsiLdErrorType::ResourceNotFound, format!("Entity {id} not found"), &uri))?;

        let mode = params.resolve_render_mode();
        let result = SnapshotRenderer::new(entity_data)
            .with_attrs_filter(params.attrs.as_deref())
            .with_mode(mode)
            .render()
            .with_entity_instance(&uri)?;

        let rendered = start_rendering(accept.media_type(), &context, prefer.rendering_preferences())
            .with_geometry_property(params.geometry_property.as_ref())
            .with_payload(ResponsePayload::Single(result))
            .render()
            .with_entity_instance(&uri)?;
        return Ok(NgsiLdResponse::ok(rendered).with_snapshot_uri(snapshot_id).into_response());
    }

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
    let local_result = service.get_inner(&id, &narrowing, &context).await;

    let is_local = params.local.unwrap_or(false);

    // If local=true, skip distributed forwarding
    let (entity, warnings) = if is_local {
        let entity = local_result.with_entity_instance(&uri)?;
        (entity, Vec::new())
    } else {
        // Forward to context sources
        let context_url = context.link_contexts().first();
        let query_string = uri.query().unwrap_or("");
        // Phase 1: forward to primary (non-auxiliary) CSRs
        let forwarded = state
            .forwarding
            .forward_retrieve(
                &id,
                query_string,
                context_url,
                ForwardingOperation::RetrieveEntity,
                incoming_via.as_ref(),
                CsrModeFilter::ExcludeAuxiliary,
            )
            .await
            .unwrap_or_default(); // Retrieve forwarding: best-effort -- missing CSR data is not fatal
        let mut warnings = collect_warnings(&forwarded);

        let local_entity = local_result.ok();

        let merged = merge_retrieve_results(local_entity, forwarded);

        // Phase 2: forward to auxiliary CSRs only if an entity was found
        // Per spec 4.3.6.2: auxiliary context sources only supplement existing data
        let merged = if merged.is_some() {
            let aux_forwarded = state
                .forwarding
                .forward_retrieve(
                    &id,
                    query_string,
                    context_url,
                    ForwardingOperation::RetrieveEntity,
                    incoming_via.as_ref(),
                    CsrModeFilter::AuxiliaryOnly,
                )
                .await
                .unwrap_or_default();
            if !aux_forwarded.is_empty() {
                warnings.extend(collect_warnings(&aux_forwarded));
                merge_retrieve_results(merged, aux_forwarded)
            } else {
                merged
            }
        } else {
            merged
        };
        let entity = match merged {
            Some(entity) => entity,
            None => {
                return Err(public_error(NgsiLdErrorType::ResourceNotFound, format!("Entity {id} not found"), &uri));
            }
        };
        (entity, warnings)
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
        let resolved = ResolvedJoin::prepare(service, from_ref(&entity), &context, &request)
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
                return Ok(NgsiLdCollectionResponse::ok(rendered).with_warnings(warnings).into_response());
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
                return Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response());
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
    Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
}
