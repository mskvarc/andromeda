//! HTTP handling for the query entity operation.
//!
//! The operation has two HTTP bindings. Clause 6.4.3.2 carries the query in a
//! query string, clause 6.23.3.1 carries it in a clause 5.2.23 body, and clause
//! 6.23.1 says of the latter that "the behaviour of this clause mirrors the one
//! in clause 6.4.3.2". They therefore differ only in how the query reaches the
//! broker: once it has been decoded, [`run`] is the operation, and both
//! bindings hand it the same [`EntityQueryRequest`].

use crate::{
    entity::{
        AcceptFormat,
        ContextSpec,
        ContextUrl,
        EntityQuery,
        EntityResultExt,
        EntityTenantState,
        GeometrySelection,
        NgsiLdProtocolHeaders,
        OriginalUri,
        ProblemResponse,
        RequestProjection,
        ResolvedJoin,
        Response,
        ResponsePayload,
        StatusCode,
        query_binding::QueryBinding,
        query_forwarding::{ForwardedQueryResults, MergedEntities, QueryForwarding},
        query_page::QueryPage,
        query_response::{QueryCollectionMetadata, query_response},
        query_split_filters::{defer_filters, retain_matching},
        request_entity_map::{LocalEntitySource, record_map_use, resolve_query_map_use},
        start_rendering,
        with_linked_resolution_warnings,
    },
    read_rendering::{EntityRepresentation, apply_representation_names, render_entities, render_joined_entities},
    state::EntityRouteServices,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::EntityQueryService;
use andromeda_federation::{
    config::SplitEntities,
    request::{EntityMapInUse, TriggeringRequest},
};
use andromeda_http_extraction::context_target;
use andromeda_http_headers::ngsild_entity_map::NgsiLdEntityMap;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::entity_selection::ConsideredEntities;
use andromeda_ngsi_ld_request::{deferred_split_filters::DeferredSplitFilters, query::entity::parameters::EntityQueryParameters};
use axum::http::{HeaderMap, Uri as HttpUri};
use std::sync::Arc;

/// One "Query Entities" as its binding decoded it.
///
/// Everything here is what the operation needs and nothing is specific to the
/// binding that supplied it: the query string binding of clause 6.4.3.2 fills
/// [`EntityQueryRequest::query`] from the URL, the POST binding of clause
/// 6.23.3.1 fills it from the clause 5.2.23 body, and from there on the two are
/// the same request.
pub(crate) struct EntityQueryRequest {
    /// The tenant-scoped capabilities the query runs against.
    pub(crate) services: Arc<EntityRouteServices>,
    /// The context branch the query reads.
    pub(crate) target: ContextTarget,
    /// The representation the response was negotiated to (clause 6.3.4).
    pub(crate) accept: AcceptFormat,
    /// The clause 6.3.x protocol headers: the `NGSILD-EntityMap` the request
    /// runs under, the `Via` chain it arrived with, and the extension headers
    /// that add to the query.
    pub(crate) protocol: NgsiLdProtocolHeaders,
    /// The URI the client addressed, which names the `instance` of every
    /// problem the operation reports and anchors its pagination links.
    pub(crate) uri: HttpUri,
    /// The request's headers, replayed onto every forwarded copy so a
    /// registration's `contextSourceInfo` can pass one of them through
    /// (clause 6.3.19).
    pub(crate) headers: HeaderMap,
    /// The JSON-LD context the query's terms expand under (clause 6.3.5).
    pub(crate) context: ContextSpec,
    /// The query itself, in the raw form the binding decoded it to.
    pub(crate) query: EntityQueryParameters,
    /// Which binding decoded it, which decides how the query is forwarded and
    /// whether the response can point at its neighbouring pages.
    pub(crate) binding: QueryBinding,
}

pub(crate) async fn query(
    context_target::ExistingTargetedTenantState { services, target, snapshot: _ }: EntityTenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormat,
    context: ContextUrl,
    protocol: NgsiLdProtocolHeaders,
    headers: HeaderMap,
    EntityQuery(query): EntityQuery,
) -> Result<Response, ProblemResponse> {
    run(EntityQueryRequest {
        services,
        target,
        accept,
        protocol,
        uri,
        headers,
        context: context.into_inner(),
        query,
        binding: QueryBinding::QueryString,
    })
    .await
}

/// Run "Query Entities" (clause 5.7.2) over a decoded request, whichever
/// binding decoded it.
///
/// # Errors
///
/// The classified failure of resolving the Entity Map the query runs under, of
/// reading the local entities, of merging what the context sources contributed,
/// of walking the clause 4.5.23 join, or of rendering the negotiated
/// representation; and the Table 6.4.3.2-3 `NotImplemented` answer when the one
/// source holding all of the data declined that representation.
pub(crate) async fn run(request: EntityQueryRequest) -> Result<Response, ProblemResponse> {
    let EntityQueryRequest {
        services: state,
        target,
        accept,
        protocol,
        uri,
        headers,
        context,
        mut query,
        binding,
    } = request;
    Box::pin(async move {
        let service = &state.entity_query;
        protocol.extensions().apply_to_entity_query(&mut query).with_entity_instance(&uri)?;
        if !target.permits_federation() {
            query.local = Some(true);
        }
        query.validate().with_entity_instance(&uri)?;
        let language_filter = query.lang.take().or(protocol.language_filter);
        let prefer = protocol.prefer;
        let incoming_via = protocol.incoming_via;
        let entity_map_header = protocol.entity_map;

        let referenced = entity_map_header.as_ref().map(NgsiLdEntityMap::as_inner);
        let map_use = resolve_query_map_use(&state.entity_maps, &target, &mut query, referenced, &uri).await?;

        let opts = query.parse_options();
        let is_local = !target.permits_federation() || query.is_local();
        let split = state.forwarding.split_entities(query.split_entities);
        let split_entities = !is_local && split == SplitEntities::Split;

        // Clause 5.5.14: an EntityMap "fixes the Entities to be considered for
        // subsequent requests based on it", so a request left considering none
        // — its identifiers and the map's do not intersect — has no entity for
        // any context source to hold, and contacting them could only widen the
        // answer the map fixes.
        let considers_nothing = matches!(ConsideredEntities::of_request(query.id.as_deref()), ConsideredEntities::NoEntity);

        // Distributed forwarding: query matching context sources unless
        // local=true. Forwarding reads the query as the client wrote it, so it
        // runs before the split-entities filters are held back from the local
        // query below.
        let forwarded = if is_local || considers_nothing {
            None
        } else {
            let forwarding = QueryForwarding {
                forwarding: &state.forwarding,
                target: &target,
                engine: service.engine(),
                uri: &uri,
                query: &query,
                split,
                context: &context,
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
                entity_map: EntityMapInUse::from(&map_use),
                operation: binding.forwarding_operation(),
            };
            Some(forwarding.run().await?)
        };

        // Clause 5.7.2.4: with `splitEntities`, `q`, the geoquery and `scopeQ`
        // are decided on the merged entities rather than by any one source.
        let deferred = split_entities.then(|| defer_filters(&mut query));
        let page = QueryPage::of(&mut query, forwarded.as_ref(), binding.page_pointers());

        let (local_entities, local_total) = service.query(&target, &query, &context).await.with_entity_instance(&uri)?;
        let MergedEntities {
            entities,
            warnings,
            contributions,
        } = merge_and_filter(service, local_entities, forwarded, deferred, &query, &context, &uri).await?;
        let (mut entities, total_count) = page.cut(entities, local_total);

        // Clause 4.21: the request's `pick`/`omit` govern both the matching
        // entities' structural members and, through their clause 4.21 nested
        // terms, the entities the traversal reaches. Both read it off the
        // compilation the query already ran, which has to outlive the join
        // that borrows its terms.
        let request_projection = RequestProjection::of(service, &query, &context).await.with_entity_instance(&uri)?;
        let projection = request_projection.projection();

        // The join resolves before the EntityMap is recorded so a minted map
        // can include the linked ids (clauses 5.5.14 and 5.7.2.4).
        let mut resolved = ResolvedJoin::of_query(service, &target, &entities, &context, &query, opts.sys_attrs, projection)
            .await
            .with_entity_instance(&uri)?;
        // Clause 6.3.17: a Linked Entity the join could not reach leaves the
        // body incomplete, and the warning header is what says so.
        let warnings = match &resolved {
            Some(resolved) => with_linked_resolution_warnings(warnings, &resolved.failures),
            None => warnings,
        };

        // Table 6.4.3.2-3 and Table 6.23.3.1-1: 201 when an EntityMap has been
        // (re)created.
        let status = if map_use.mints() { StatusCode::CREATED } else { StatusCode::OK };
        let map_entities = LocalEntitySource::queried(service, &target, &query, &context, resolved.as_ref(), &uri);
        let entity_map_uri = record_map_use(&state.entity_maps, map_use, contributions, map_entities, &uri).await?;

        let representation = EntityRepresentation::new(language_filter.as_ref(), opts.render_mode, projection);
        apply_representation_names(service, &representation, &context, &mut entities, resolved.as_mut(), &uri).await?;
        let entities = entities;
        let payload = match &resolved {
            Some(resolved) => render_joined_entities(&entities, resolved, &representation, &uri)?,
            None => render_entities(&entities, &representation, &uri)?,
        };
        let geometry = GeometrySelection::new(query.geometry_property.as_ref(), query.dataset_id.as_deref().unwrap_or_default());
        let rendered = start_rendering(accept.media_type(), &context, prefer.rendering_preferences(), opts.render_mode)
            .with_geometry_selection(geometry)
            .with_payload(ResponsePayload::Collection(payload))
            .render()
            .with_entity_instance(&uri)?;
        Ok(query_response(
            rendered,
            QueryCollectionMetadata {
                status,
                warnings,
                total_count,
                entity_map_uri,
                returned: entities.len(),
            },
            &page,
            accept.media_type(),
            &uri,
        ))
    })
    .await
}

/// Fold what the context sources contributed onto the entities this broker
/// holds itself, then apply the filters `splitEntities` held back.
///
/// Clause 5.7.2.4: with `splitEntities` no single source held enough of an
/// entity to decide `q`, the geoquery or `scopeQ`, so those are evaluated once
/// here, over the merged result. A query that deferred nothing keeps every
/// entity the merge produced.
///
/// # Errors
///
/// The classified failure of compiling a deferred filter against the request's
/// `@context`.
async fn merge_and_filter(
    service: &EntityQueryService,
    local_entities: Vec<Entity>,
    forwarded: Option<ForwardedQueryResults>,
    deferred: Option<DeferredSplitFilters>,
    query: &EntityQueryParameters,
    context: &ContextSpec,
    instance: &HttpUri,
) -> Result<MergedEntities, ProblemResponse> {
    let MergedEntities {
        entities,
        warnings,
        contributions,
    } = match forwarded {
        Some(results) => results.merge_onto(local_entities, query.attrs.as_deref()),
        None => MergedEntities::local(local_entities),
    };
    let entities = match deferred {
        Some(deferred) => retain_matching(service, entities, deferred, context, instance).await?,
        None => entities,
    };
    Ok(MergedEntities {
        entities,
        warnings,
        contributions,
    })
}
