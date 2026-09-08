//! HTTP handling for the query entity operation.

use crate::{
    entity_operations::{
        AcceptFormat,
        ContextSpec,
        ContextUrl,
        CsrModeFilter,
        EntityQueryParameters,
        EntityQueryService,
        EntityResultExt,
        EntityTenantState,
        ForwardQueryRequest,
        ForwardingOperation,
        IntoResponse,
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
        ValidatedQuery,
        collect_warnings,
        start_rendering,
        with_linked_resolution_warnings,
    },
    read_rendering::{EntityRepresentation, render_entities, render_joined_entities},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_federation::{
    FederationService,
    forwarded_query::ClientQuery,
    outcome::ForwardReadOutcome,
    request::{EntityMapInUse, TriggeringRequest},
};
use andromeda_http_extraction::context_target;
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_model::entity::Entity;
use axum::http::HeaderMap;

/// The entities the matching context sources contributed to a batch query,
/// merged onto the ones this broker holds itself, with the clause 6.3.17
/// warnings the sources raised.
///
/// Forwarding is best-effort: a source that could not be reached leaves the
/// operation standing on what the remaining sources answered.
async fn merged_with_sources(
    forwarding: &FederationService,
    target: &ContextTarget,
    params: &EntityQueryParameters,
    context: &ContextSpec,
    triggering: TriggeringRequest<'_>,
    local_entities: Vec<Entity>,
) -> (Vec<Entity>, Vec<NgsiLdWarning>) {
    let forwarded = forwarding
        .forward_query(
            target,
            ForwardQueryRequest {
                client_query: ClientQuery::EntityQuery(params),
                split: forwarding.split_entities(params.split_entities),
                context,
                operation: ForwardingOperation::QueryBatch,
                triggering,
                csf_expr: None, // csf not supported on batch query endpoint
                mode_filter: CsrModeFilter::All,
                entity_map: EntityMapInUse::None,
                source_page: None,
            },
        )
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(error = %error, "Context source forwarding failed during batch query");
            ForwardReadOutcome::empty()
        })
        .results;
    let warnings = collect_warnings(&forwarded);
    if forwarded.is_empty() {
        (local_entities, warnings)
    } else {
        (EntityQueryService::merge_with_forwarded(local_entities, forwarded, None), warnings)
    }
}

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
        let triggering = TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        };
        merged_with_sources(&state.forwarding, &target, &params, &context, triggering, local_entities).await
    };

    // Clause 4.21: the request's `pick`/`omit` govern both the matching
    // entities' structural members and, through their clause 4.21 nested
    // terms, the entities the traversal reaches. Both read it off the
    // compilation the query already ran.
    let request_projection = RequestProjection::of(service, &params, &context).await.with_entity_instance(&uri)?;
    let projection = request_projection.projection();

    let resolved = ResolvedJoin::of_query(service, &target, &entities, &context, &params, opts.sys_attrs, projection)
        .await
        .with_entity_instance(&uri)?;
    // Clause 6.3.17: a Linked Entity the join could not reach leaves the
    // body incomplete, and the warning header is what says so.
    let warnings = match &resolved {
        Some(resolved) => with_linked_resolution_warnings(warnings, &resolved.failures),
        None => warnings,
    };

    let representation = EntityRepresentation::new(language_filter.as_ref(), opts.render_mode, projection);
    let payload = match &resolved {
        Some(resolved) => render_joined_entities(&entities, resolved, &representation, &uri)?,
        None => render_entities(&entities, &representation, &uri)?,
    };
    let rendered = start_rendering(accept.media_type(), &context, prefer.rendering_preferences())
        .with_geometry_property(params.geometry_property.as_ref())
        .with_payload(ResponsePayload::Collection(payload))
        .render()
        .with_entity_instance(&uri)?;
    let mut response = NgsiLdCollectionResponse::ok(rendered).with_warnings(warnings);
    if let Some(count) = total_count
        && let Ok(count) = usize::try_from(count)
    {
        response = response.with_total_count(count);
    }
    Ok(response.into_response())
}
