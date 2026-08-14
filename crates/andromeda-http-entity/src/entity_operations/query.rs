//! HTTP handling for the query entity operation.

use crate::entity_operations::{
    AcceptFormat,
    ContextUrl,
    CsrModeFilter,
    EntityQueryParameters,
    EntityQueryService,
    EntityRenderer,
    EntityResultExt,
    EntityTenantState,
    ForwardQueryRequest,
    ForwardingOperation,
    IntoResponse,
    JoinRequest,
    JsonLd,
    NgsiLdCollectionResponse,
    NgsiLdProtocolHeaders,
    OriginalUri,
    PostQueryParams,
    PreferHeader,
    ProblemResponse,
    Query,
    RequestProjection,
    ResolvedJoin,
    Response,
    ResponsePayload,
    RetrievingJoin,
    SnapshotHeader,
    ValidatedQuery,
    Value,
    collect_warnings,
    render_flat_join,
    start_rendering,
    with_linked_resolution_warnings,
};
use std::iter::empty;

pub(crate) async fn query(
    andromeda_http_extraction::tenant_state::ExistingTenantState(state): EntityTenantState,
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
) -> Result<Response, ProblemResponse> {
    let service = &state.entity_query;
    let context = context.merge(body_context);

    let mut params = EntityQueryParameters::try_from(request).with_entity_instance(&uri)?;

    // ETSI GS CIM 009 clause 5.5.15: when NGSILD-Snapshot is set, serve frozen
    // data and skip both the live pipeline and CSR forwarding.
    if let Some(snapshot_id) = snapshot_id {
        state.snapshots.validate(&snapshot_id).await.with_entity_instance(&uri)?;
        let entity_type = params.entity_type.as_ref();
        let entity_ids = params.id.clone();
        let limit = i64::from(params.limit.unwrap_or(1000));
        let offset = i64::from(params.offset.unwrap_or(0));
        let (entities, _total) = state
            .snapshots
            .entities(&snapshot_id, entity_type, entity_ids.as_deref(), limit, offset)
            .await
            .with_entity_instance(&uri)?;
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
            .with_payload(ResponsePayload::Collection(entities))
            .render()
            .with_entity_instance(&uri)?;
        return Ok(NgsiLdCollectionResponse::ok(rendered).into_response());
    }
    // The same parameter may be written in the URL or in the canonical body.
    // The specification does not say which wins, so this is broker
    // convention: the URL is the more specific of the two and takes
    // precedence, but only where it actually carries a value -- an absent
    // query-string parameter leaves the body's member standing rather than
    // erasing it.
    params.options = qparams.options.or(params.options);
    params.local = qparams.local.or(params.local);
    params.pick = qparams.pick.or(params.pick);
    params.omit = qparams.omit.or(params.omit);
    params.dataset_id = qparams.dataset_id.or(params.dataset_id);
    params.format = qparams.format.or(params.format);
    params.join = qparams.join.or(params.join);
    params.join_level = qparams.join_level.or(params.join_level);
    params.geometry_property = qparams.geometry_property.or(params.geometry_property);
    protocol.extensions().apply_to_entity_query(&mut params).with_entity_instance(&uri)?;
    let language_filter = qparams.lang.or_else(|| params.lang.take()).or(protocol.language_filter);
    let incoming_via = protocol.incoming_via;
    params.validate().with_entity_instance(&uri)?;

    let opts = params.parse_options();

    let is_local = params.local.unwrap_or(false);
    let (local_entities, total_count) = service.query(&params, &context).await.with_entity_instance(&uri)?;

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

    // Clause 4.21: the request's `pick`/`omit` govern both the matching
    // entities' structural members and, through their clause 4.21 nested
    // terms, the entities the traversal reaches. Both read it off the
    // compilation the query already ran.
    let request_projection = RequestProjection::of(service, &params, &context).await.with_entity_instance(&uri)?;
    let projection = request_projection.projection();

    // Join mode (clause 4.5.23) is parsed at the query boundary. `@none` and
    // an absent `join` retrieve no linked entity at all, so neither reaches
    // the traversal.
    if let Some(join) = RetrievingJoin::from_mode(params.join) {
        let request = JoinRequest {
            join,
            join_level: params.join_level,
            contained_by: params.contained_by.as_deref(),
            sys_attrs: opts.sys_attrs,
            projection,
        };
        let resolved = ResolvedJoin::prepare(service, &entities, &context, &request).await.with_entity_instance(&uri)?;
        // Clause 6.3.17: a Linked Entity the join could not reach leaves the
        // body incomplete, and the warning header is what says so.
        let warnings = with_linked_resolution_warnings(warnings, &resolved.failures);

        let payload = match join {
            // Flat: the root entities and their linked entities side by side,
            // the roots ahead of the linked (clause 4.5.23).
            RetrievingJoin::Flat => render_flat_join(
                entities.iter(),
                resolved.linked.values(),
                language_filter.as_ref(),
                opts.render_mode,
                projection,
            )
            .with_entity_instance(&uri)?,
            // Inline: the renderer nests the linked entities into each root,
            // narrowing each nested copy by the sub-projection its own
            // Relationship carried (clause 4.21).
            RetrievingJoin::Inline => {
                let mut entity_values: Vec<Value> = Vec::with_capacity(entities.len());
                for entity in &entities {
                    entity_values.push(
                        EntityRenderer::new(entity)
                            .with_optional_language_filter(language_filter.as_ref())
                            .with_inline_linked_entities(&resolved.linked)
                            .with_optional_linked_projection(resolved.linked_projection())
                            .with_optional_structural_projection(projection)
                            .with_mode(opts.render_mode)
                            .render()
                            .with_entity_instance(&uri)?,
                    );
                }
                entity_values
            }
        };

        let rendered = start_rendering(accept.media_type(), &context, protocol.prefer.rendering_preferences())
            .with_geometry_property(params.geometry_property.as_ref())
            .with_payload(ResponsePayload::Collection(payload))
            .render()
            .with_entity_instance(&uri)?;
        let mut response = NgsiLdCollectionResponse::ok(rendered).with_warnings(warnings);
        if let Some(count) = total_count {
            response = response.with_total_count(count as usize);
        }
        return Ok(response.into_response());
    }

    let entity_values = render_flat_join(entities.iter(), empty(), language_filter.as_ref(), opts.render_mode, projection).with_entity_instance(&uri)?;
    let rendered = start_rendering(accept.media_type(), &context, protocol.prefer.rendering_preferences())
        .with_geometry_property(params.geometry_property.as_ref())
        .with_payload(ResponsePayload::Collection(entity_values))
        .render()
        .with_entity_instance(&uri)?;
    let mut response = NgsiLdCollectionResponse::ok(rendered).with_warnings(warnings);
    if let Some(count) = total_count {
        response = response.with_total_count(count as usize);
    }
    Ok(response.into_response())
}
