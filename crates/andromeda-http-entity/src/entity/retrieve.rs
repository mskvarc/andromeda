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
    HashSet,
    IntoResponse,
    JoinMode,
    NgsiLdCollectionResponse,
    NgsiLdErrorType,
    NgsiLdProtocolHeaders,
    NgsiLdResponse,
    NonZeroU32,
    OriginalUri,
    ProblemResponse,
    Response,
    ResponsePayload,
    SnapshotRenderer,
    TypedPath,
    Uri,
    ValidatedQuery,
    collect_warnings,
    merge_retrieve_results,
    public_error,
    render_flat_join,
    start_rendering,
};

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
    // `format` or `options` returns 400, not 404 (clause 4.5.1).
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

    let attrs = params.attrs.as_deref().unwrap_or_default();

    let sys_attrs = params.options.unwrap_or_default().sys_attrs;
    let dataset_ids = params.dataset_id.as_deref().unwrap_or_default();
    let local_result = service.get_inner(&id, attrs, sys_attrs, dataset_ids, &context).await;

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

    // Join mode (clause 4.5.23) is parsed at the query boundary.
    let join_mode = params.join;
    let join_level = params.join_level.map_or(1, NonZeroU32::get);

    let contained_by: HashSet<Uri> = params.contained_by.as_deref().map(|ids| ids.iter().cloned().collect()).unwrap_or_default();

    if let Some(join) = join_mode {
        let linked = service
            .resolve_linked_entities(std::slice::from_ref(&entity), join_level, sys_attrs, &context, &contained_by)
            .await
            .with_entity_instance(&uri)?;

        match join {
            JoinMode::Flat => {
                // Flat: the root and its linked entities side by side (clause 4.5.23).
                let all_entities =
                    render_flat_join(std::iter::once(&entity).chain(linked.values()), language_filter.as_ref(), mode).with_entity_instance(&uri)?;
                let rendered = start_rendering(accept.media_type(), &context, prefer.rendering_preferences())
                    .with_payload(ResponsePayload::Collection(all_entities))
                    .render()
                    .with_entity_instance(&uri)?;
                return Ok(NgsiLdCollectionResponse::ok(rendered).with_warnings(warnings).into_response());
            }
            JoinMode::Inline => {
                // Inline: the renderer nests the linked entities into the root.
                let entity_value = EntityRenderer::new(&entity)
                    .with_optional_language_filter(language_filter.as_ref())
                    .with_inline_linked_entities(&linked)
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
            JoinMode::None => {}
        }
    }

    let entity_value = EntityRenderer::new(&entity)
        .with_optional_language_filter(language_filter.as_ref())
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
