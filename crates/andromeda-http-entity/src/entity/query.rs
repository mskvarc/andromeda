//! HTTP handling for the query entity operation.

use crate::{
    entity::{
        AcceptFormat,
        ContextSpec,
        ContextUrl,
        EntityQuery,
        EntityResultExt,
        EntityTenantState,
        IntoResponse,
        NgsiLdCollectionResponse,
        NgsiLdProtocolHeaders,
        OriginalUri,
        ProblemResponse,
        RequestProjection,
        ResolvedJoin,
        Response,
        ResponsePayload,
        StatusCode,
        query_forwarding::{MergedEntities, QueryForwarding},
        query_page::QueryPage,
        query_split_filters::{defer_filters, retain_matching},
        request_entity_map::{LocalEntitySource, record_map_use, resolve_query_map_use},
        start_rendering,
        with_linked_resolution_warnings,
    },
    read_rendering::{EntityRepresentation, render_entities, render_joined_entities},
};
use andromeda_federation::{
    config::SplitEntities,
    request::{EntityMapInUse, TriggeringRequest},
};
use andromeda_http_extraction::context_target;
use andromeda_http_headers::ngsild_entity_map::NgsiLdEntityMap;
use andromeda_ngsi_ld_query::entity_selection::ConsideredEntities;
use axum::http::HeaderMap;

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
    EntityQuery(mut query): EntityQuery,
) -> Result<Response, ProblemResponse> {
    Box::pin(async move {
        let service = &state.entity_query;
        let context: ContextSpec = context.into_inner();
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
            };
            Some(forwarding.run().await?)
        };

        // Clause 5.7.2.4: with `splitEntities`, `q`, the geoquery and `scopeQ`
        // are decided on the merged entities rather than by any one source.
        let deferred = split_entities.then(|| defer_filters(&mut query));
        let page = QueryPage::of(&mut query, forwarded.as_ref());

        let (local_entities, local_total) = service.query(&target, &query, &context).await.with_entity_instance(&uri)?;
        let MergedEntities {
            entities,
            warnings,
            contributions,
        } = match forwarded {
            Some(results) => results.merge_onto(local_entities, query.attrs.as_deref()),
            None => MergedEntities::local(local_entities),
        };
        let entities = match deferred {
            Some(deferred) => retain_matching(service, entities, deferred, &context, &uri).await?,
            None => entities,
        };
        let (entities, total_count) = page.cut(entities, local_total);

        // Clause 4.21: the request's `pick`/`omit` govern both the matching
        // entities' structural members and, through their clause 4.21 nested
        // terms, the entities the traversal reaches. Both read it off the
        // compilation the query already ran, which has to outlive the join
        // that borrows its terms.
        let request_projection = RequestProjection::of(service, &query, &context).await.with_entity_instance(&uri)?;
        let projection = request_projection.projection();

        // The join resolves before the EntityMap is recorded so a minted map
        // can include the linked ids (clauses 5.5.14 and 5.7.2.4).
        let resolved = ResolvedJoin::of_query(service, &target, &entities, &context, &query, opts.sys_attrs, projection)
            .await
            .with_entity_instance(&uri)?;
        // Clause 6.3.17: a Linked Entity the join could not reach leaves the
        // body incomplete, and the warning header is what says so.
        let warnings = match &resolved {
            Some(resolved) => with_linked_resolution_warnings(warnings, &resolved.failures),
            None => warnings,
        };

        // Table 6.4.3.2-3: 201 when an EntityMap has been (re)created.
        let status = if map_use.mints() { StatusCode::CREATED } else { StatusCode::OK };
        let map_entities = LocalEntitySource::Queried {
            service,
            target: &target,
            query: &query,
            context: &context,
            join: resolved.as_ref(),
            instance: &uri,
        };
        let entity_map_uri = record_map_use(&state.entity_maps, map_use, contributions, map_entities, &uri).await?;

        let representation = EntityRepresentation::new(language_filter.as_ref(), opts.render_mode, projection);
        let payload = match &resolved {
            Some(resolved) => render_joined_entities(&entities, resolved, &representation, &uri)?,
            None => render_entities(&entities, &representation, &uri)?,
        };
        let rendered = start_rendering(accept.media_type(), &context, prefer.rendering_preferences())
            .with_geometry_property(query.geometry_property.as_ref())
            .with_payload(ResponsePayload::Collection(payload))
            .render()
            .with_entity_instance(&uri)?;
        let mut response = NgsiLdCollectionResponse::ok(rendered).with_status(status).with_warnings(warnings);
        if let Some(count) = total_count
            && let Ok(count) = usize::try_from(count)
        {
            response = response.with_total_count(count);
        }
        if let Some(map_uri) = entity_map_uri {
            response = response.with_entity_map_uri(map_uri);
        }
        let mut response = response.into_response();
        page.append_links(&mut response, &uri, accept.media_type(), total_count, entities.len());
        Ok(response)
    })
    .await
}
