//! HTTP handling for the retrieve entity operation.

use crate::http::handler::entity::{
    AcceptFormat,
    AndromedaError,
    AttributeName,
    ContextSpec,
    ContextUrl,
    CsrModeFilter,
    EntityRenderer,
    ForwardingOperation,
    GetEntityParams,
    HashSet,
    IntoResponse,
    JoinMode,
    NgsiLdCollectionResponse,
    NgsiLdError,
    NgsiLdProtocolHeaders,
    NgsiLdResponse,
    NonZeroU32,
    OriginalUri,
    Path,
    Response,
    ResponsePayload,
    ResultInstanceExt,
    SnapshotRenderer,
    TenantState,
    Uri,
    ValidatedQuery,
    collect_warnings,
    merge_retrieve_results,
    parse_entity_id,
    render_flat_join,
    start_rendering,
};

pub async fn get(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    Path(id): Path<String>,
    ValidatedQuery(mut params): ValidatedQuery<GetEntityParams>,
    protocol: NgsiLdProtocolHeaders,
) -> Result<Response, NgsiLdError> {
    let service = &state.entity_query;
    let id = parse_entity_id(&id, &uri)?;
    let context: ContextSpec = context.into_inner();
    protocol
        .extensions()
        .apply_to_entity_get(&mut params)
        .map_err(|error| AndromedaError::from(error).with_instance(&uri))?;
    let language_filter = params.lang.take().or(protocol.language_filter);
    let prefer = protocol.prefer;
    let snapshot = protocol.snapshot;
    let incoming_via = protocol.incoming_via;

    // Validate the representation parameters before entity lookup so an invalid
    // `format` or `options` returns 400, not 404 (clause 4.5.1).
    params.validate().with_instance(&uri).map_err(NgsiLdError::from)?;

    // Snapshot scoping (ETSI GS CIM 009 clause 5.5.15, 6.3.22): if NGSILD-Snapshot header
    // is present, retrieve entity from the snapshot's frozen data and return early.
    if let Some(snapshot_id) = snapshot.into_inner() {
        let _snap = state.snapshots.validate_for_scoping(&snapshot_id).await.with_instance(&uri)?;

        let entity_uri = Uri::from(id.clone()); // clone: owned Urn -> Uri; the snapshot store is keyed by Uri
        let entity_data = state
            .snapshots
            .get_snapshot_entity(&snapshot_id, &entity_uri)
            .await
            .with_instance(&uri)?
            .ok_or_else(|| NgsiLdError::from(AndromedaError::entity_not_found(id.to_string()).with_instance(&uri)))?;

        let mode = params.resolve_render_mode();
        let attrs: Option<Vec<&str>> = params.attrs.as_deref().map(|names| names.iter().map(AttributeName::as_str).collect());
        let result = SnapshotRenderer::new(entity_data)
            .with_attrs_filter(attrs.as_deref())
            .with_mode(mode)
            .render()?;

        let rendered = start_rendering(accept.media_type(), &context, prefer)
            .with_geometry_property(params.geometry_property.as_ref().map(AttributeName::as_str))
            .with_payload(ResponsePayload::Single(result))
            .render()?;
        return Ok(NgsiLdResponse::ok(rendered).with_snapshot_uri(snapshot_id).into_response());
    }

    let attrs: Vec<String> = params
        .attrs
        .as_deref()
        .map(|names| names.iter().map(ToString::to_string).collect())
        .unwrap_or_default();

    let sys_attrs = params.options.unwrap_or_default().sys_attrs;
    let dataset_ids: Vec<String> = params
        .dataset_id
        .as_deref()
        .map(|ids| ids.iter().map(ToString::to_string).collect())
        .unwrap_or_default();
    let local_result = service.get_inner(&id, &attrs, sys_attrs, &dataset_ids, &context).await;

    let is_local = params.local.unwrap_or(false);

    // If local=true, skip distributed forwarding
    let (entity, warnings) = if is_local {
        let entity = local_result.with_instance(&uri)?;
        (entity, Vec::new())
    } else {
        // Forward to context sources
        let context_url = context.link_contexts().first().map(|s| s.as_str());
        let query_string = uri.query().unwrap_or("");
        // Phase 1: forward to primary (non-auxiliary) CSRs
        let forwarded = state
            .forwarding
            .forward_retrieve(
                id.as_ref(),
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
                    id.as_ref(),
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
                return Err(NgsiLdError::from(AndromedaError::entity_not_found(id.to_string()).with_instance(&uri)));
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
            .with_instance(&uri)?;

        match join {
            JoinMode::Flat => {
                // Flat: the root and its linked entities side by side (clause 4.5.23).
                let all_entities = render_flat_join(std::iter::once(&entity).chain(linked.values()), language_filter.as_ref(), mode).with_instance(&uri)?;
                let rendered = start_rendering(accept.media_type(), &context, prefer)
                    .with_payload(ResponsePayload::Collection(all_entities))
                    .render()?;
                return Ok(NgsiLdCollectionResponse::ok(rendered).with_warnings(warnings).into_response());
            }
            JoinMode::Inline => {
                // Inline: the renderer nests the linked entities into the root.
                let entity_value = EntityRenderer::new(&entity)
                    .with_optional_language_filter(language_filter.as_ref())
                    .with_inline_linked_entities(&linked)
                    .with_mode(mode)
                    .render()
                    .with_instance(&uri)?;
                let rendered = start_rendering(accept.media_type(), &context, prefer)
                    .with_geometry_property(params.geometry_property.as_ref().map(AttributeName::as_str))
                    .with_payload(ResponsePayload::Single(entity_value))
                    .render()?;
                return Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response());
            }
            JoinMode::None => {}
        }
    }

    let entity_value = EntityRenderer::new(&entity)
        .with_optional_language_filter(language_filter.as_ref())
        .with_mode(mode)
        .render()
        .with_instance(&uri)?;
    let rendered = start_rendering(accept.media_type(), &context, prefer)
        .with_geometry_property(params.geometry_property.as_ref().map(AttributeName::as_str))
        .with_payload(ResponsePayload::Single(entity_value))
        .render()?;
    Ok(NgsiLdResponse::ok(rendered).with_warnings(warnings).into_response())
}
