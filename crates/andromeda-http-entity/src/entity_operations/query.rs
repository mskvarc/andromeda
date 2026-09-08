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
    ProblemResponse,
    Query,
    RequestProjection,
    ResolvedJoin,
    Response,
    ResponsePayload,
    RetrievingJoin,
    ValidatedQuery,
    Value,
    collect_warnings,
    render_flat_join,
    start_rendering,
    with_linked_resolution_warnings,
};
use andromeda_federation::{
    forwarded_query::ClientQuery,
    outcome::ForwardReadOutcome,
    request::{EntityMapInUse, TriggeringRequest},
};
use andromeda_http_extraction::context_target;
use axum::http::HeaderMap;
use std::iter::empty;

pub(crate) async fn query(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: EntityTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    protocol: NgsiLdProtocolHeaders,
    headers: HeaderMap,
    ValidatedQuery(qparams): ValidatedQuery<PostQueryParams>,
    JsonLd {
        value: request,
        context: body_context,
    }: JsonLd<Query>,
) -> Result<Response, ProblemResponse> {
    let service = &state.entity_query;
    let context = context.merge(body_context);

    let mut params = EntityQueryParameters::try_from(request).with_entity_instance(&uri)?;

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
    let prefer = protocol.prefer;

    params.validate().with_entity_instance(&uri)?;

    let opts = params.parse_options();

    let is_local = !target.permits_federation() || params.is_local();
    let (local_entities, total_count) = service.query(&target, &params, &context).await.with_entity_instance(&uri)?;

    // Distributed forwarding: query matching context sources unless local=true
    let (entities, warnings) = if is_local {
        (local_entities, Vec::new())
    } else {
        let forwarded = state
            .forwarding
            .forward_query(
                &target,
                ForwardQueryRequest {
                    client_query: ClientQuery::EntityQuery(&params),
                    split: state.forwarding.split_entities(params.split_entities),
                    context: &context,
                    operation: ForwardingOperation::QueryBatch,
                    triggering: TriggeringRequest {
                        via: incoming_via.as_ref(),
                        headers: Some(&headers),
                    },
                    csf_expr: None, // csf not supported on batch query endpoint
                    mode_filter: CsrModeFilter::All,
                    entity_map: EntityMapInUse::None,
                    source_page: None,
                },
            )
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "Context source forwarding failed during batch query");
                ForwardReadOutcome::empty()
            })
            .results;
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
        let resolved = ResolvedJoin::prepare(service, &target, &entities, &context, &request)
            .await
            .with_entity_instance(&uri)?;
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

        let rendered = start_rendering(accept.media_type(), &context, prefer.rendering_preferences())
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
    let rendered = start_rendering(accept.media_type(), &context, prefer.rendering_preferences())
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
