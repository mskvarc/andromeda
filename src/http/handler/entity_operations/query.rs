//! HTTP handling for the query entity operation.

use crate::http::handler::entity_operations::{
    AcceptFormat,
    AndromedaError,
    ContextUrl,
    CsrModeFilter,
    EntityQueryParameters,
    EntityQueryService,
    EntityRenderer,
    ForwardQueryRequest,
    ForwardingOperation,
    HashSet,
    IntoResponse,
    JoinMode,
    JsonLd,
    NgsiLdCollectionResponse,
    NgsiLdError,
    NgsiLdProtocolHeaders,
    NonZeroU32,
    OriginalUri,
    PostQueryParams,
    PreferHeader,
    Query,
    Response,
    ResponsePayload,
    ResultInstanceExt,
    SnapshotHeader,
    TenantState,
    ValidatedQuery,
    Value,
    collect_warnings,
    start_rendering,
};

pub async fn query(
    TenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    protocol: NgsiLdProtocolHeaders,
    SnapshotHeader(snapshot_id): SnapshotHeader,
    ValidatedQuery(qparams): ValidatedQuery<PostQueryParams>,
    JsonLd {
        value: request,
        context: body_context,
    }: JsonLd<Query>,
) -> Result<Response, NgsiLdError> {
    let service = &state.entity_query;
    let context = context.merge(body_context);

    let mut params = EntityQueryParameters::try_from(request)
        .map_err(AndromedaError::from)
        .with_instance(&uri)
        .map_err(NgsiLdError::from)?;

    // ETSI GS CIM 009 clause 5.5.15: when NGSILD-Snapshot is set, serve frozen
    // data and skip both the live pipeline and CSR forwarding.
    if let Some(snapshot_id) = snapshot_id {
        state.snapshots.validate_for_scoping(&snapshot_id).await.with_instance(&uri)?;
        let entity_type = params.entity_type.as_ref();
        let entity_ids = params.id.clone();
        let limit = i64::from(params.limit.unwrap_or(1000));
        let offset = i64::from(params.offset.unwrap_or(0));
        let (entities, _total) = state
            .snapshots
            .query_snapshot_entities(&snapshot_id, entity_type, entity_ids.as_deref(), limit, offset)
            .await
            .with_instance(&uri)?;
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default())
            .with_payload(ResponsePayload::Collection(entities))
            .render()?;
        return Ok(NgsiLdCollectionResponse::ok(rendered).into_response());
    }
    // Merge fields from query string parameters
    params.options = qparams.options;
    params.local = qparams.local;
    params.pick = qparams.pick;
    params.omit = qparams.omit;
    params.dataset_id = qparams.dataset_id;
    params.format = qparams.format;
    params.join = qparams.join;
    params.join_level = qparams.join_level;
    params.geometry_property = qparams.geometry_property;
    protocol
        .extensions()
        .apply_to_entity_query(&mut params)
        .map_err(|error| AndromedaError::from(error).with_instance(&uri))?;
    let language_filter = qparams.lang.or_else(|| params.lang.take()).or(protocol.language_filter);
    let incoming_via = protocol.incoming_via;
    params.validate().with_instance(&uri).map_err(NgsiLdError::from)?;

    let opts = params.parse_options();
    let join_mode = params.join;
    let join_level = params.join_level.map_or(1, NonZeroU32::get);
    let geometry_property = params.geometry_property.as_ref();

    let is_local = params.local.unwrap_or(false);
    let (local_entities, total_count) = service.query(&params, &context).await.with_instance(&uri)?;

    // Distributed forwarding: query matching context sources unless local=true
    let (entities, warnings) = if is_local {
        (local_entities, Vec::new())
    } else {
        let context_url = context.link_contexts().first();
        let entity_type = params.entity_type.as_ref();
        let entity_ids = params.id.clone();
        let entity_ids_ref = entity_ids.as_deref();

        let forwarded = state
            .forwarding
            .forward_query(ForwardQueryRequest {
                entity_type_compact: entity_type,
                entity_ids: entity_ids_ref,
                context_url,
                context: &context,
                operation: ForwardingOperation::QueryBatch,
                incoming_via: incoming_via.as_ref(),
                csf_expr: None, // csf not supported on batch query endpoint
                mode_filter: CsrModeFilter::All,
            })
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "Context source forwarding failed during batch query");
                Vec::new()
            });
        let warnings = collect_warnings(&forwarded);

        if forwarded.is_empty() {
            (local_entities, warnings)
        } else {
            let merged = EntityQueryService::merge_with_forwarded(local_entities, forwarded, None);
            (merged, warnings)
        }
    };

    // Handle join
    if join_mode.is_some() {
        let linked = service
            .resolve_linked_entities(&entities, join_level, opts.sys_attrs, &context, &HashSet::new())
            .await
            .with_instance(&uri)?;

        if join_mode == Some(JoinMode::Flat) {
            let mut all_values: Vec<Value> = Vec::new();
            for entity in &entities {
                all_values.push(
                    EntityRenderer::new(entity)
                        .with_optional_language_filter(language_filter.as_ref())
                        .with_mode(opts.render_mode)
                        .render()
                        .with_instance(&uri)?,
                );
            }
            for linked_entity in linked.values() {
                all_values.push(
                    EntityRenderer::new(linked_entity)
                        .with_optional_language_filter(language_filter.as_ref())
                        .with_mode(opts.render_mode)
                        .render()
                        .with_instance(&uri)?,
                );
            }
            let rendered = start_rendering(accept.media_type(), &context, protocol.prefer)
                .with_geometry_property(geometry_property)
                .with_payload(ResponsePayload::Collection(all_values))
                .render()?;
            let mut response = NgsiLdCollectionResponse::ok(rendered).with_warnings(warnings);
            if let Some(count) = total_count {
                response = response.with_total_count(count as usize);
            }
            return Ok(response.into_response());
        } else {
            // Inline: renderer handles join + mode transformation
            let mut entity_values: Vec<Value> = Vec::new();
            for entity in &entities {
                entity_values.push(
                    EntityRenderer::new(entity)
                        .with_optional_language_filter(language_filter.as_ref())
                        .with_inline_linked_entities(&linked)
                        .with_mode(opts.render_mode)
                        .render()
                        .with_instance(&uri)?,
                );
            }
            let rendered = start_rendering(accept.media_type(), &context, protocol.prefer)
                .with_geometry_property(geometry_property)
                .with_payload(ResponsePayload::Collection(entity_values))
                .render()?;
            let mut response = NgsiLdCollectionResponse::ok(rendered).with_warnings(warnings);
            if let Some(count) = total_count {
                response = response.with_total_count(count as usize);
            }
            return Ok(response.into_response());
        }
    }

    let entity_values = entities
        .iter()
        .map(|entity| {
            EntityRenderer::new(entity)
                .with_optional_language_filter(language_filter.as_ref())
                .with_mode(opts.render_mode)
                .render()
        })
        .collect::<Result<Vec<_>, _>>()
        .with_instance(&uri)?;
    let rendered = start_rendering(accept.media_type(), &context, protocol.prefer)
        .with_geometry_property(geometry_property)
        .with_payload(ResponsePayload::Collection(entity_values))
        .render()?;
    let mut response = NgsiLdCollectionResponse::ok(rendered).with_warnings(warnings);
    if let Some(count) = total_count {
        response = response.with_total_count(count as usize);
    }
    Ok(response.into_response())
}
