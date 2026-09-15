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
        query_split_filters::retain_matching,
        request_entity_map::{LocalEntitySource, record_map_use, resolve_query_map_use},
        start_rendering,
        with_linked_resolution_warnings,
    },
    read_rendering::{EntityRepresentation, apply_representation_names, render_entities, render_joined_entities},
    state::EntityRouteServices,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::EntityQueryService;
use andromeda_entity_map::{recording::PageMatches, reuse::EntityMapUse};
use andromeda_federation::{
    FederationService,
    config::SplitEntities,
    forwarded_query::client_query::ClientQuery,
    ordering_admission::{OrderedRead, OrderingAdmission},
    request::{EntityMapInUse, TriggeringRequest},
};
use andromeda_http_extraction::{context_target, prefer::PreferHeader};
use andromeda_http_headers::{language::LanguageFilter, ngsild_entity_map::NgsiLdEntityMap, via::Via};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::entity_selection::ConsideredEntities;
use andromeda_ngsi_ld_request::{
    deferred_split_filters::DeferredSplitFilters,
    error::ParameterError,
    query::entity::parameters::{EntityQueryParameters, EntityQueryParametersOptions},
};
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
        let AdmittedQuery {
            language_filter,
            prefer,
            incoming_via,
            map_use,
            opts,
        } = admit_request(&state, &target, &context, protocol, &mut query, binding, &uri).await?;

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
        let deferred = split_entities.then(|| query.defer_split_filters());
        let page = QueryPage::of(&mut query, forwarded.as_ref(), binding.page_pointers());

        let (local_entities, local_total) = service.query(&target, &query, &context).await.with_entity_instance(&uri)?;
        let MergedEntities {
            entities,
            warnings,
            contributions,
        } = merge_and_filter(service, local_entities, forwarded, deferred, &query, &context, &uri).await?;
        // Clause 5.5.9.3: under `splitEntities` the entities that survived the
        // deferred filters over the merged result are the whole match set the
        // reused map is pruned against -- captured here over the full result,
        // before the page window narrows it. A non-split page prunes per source.
        let split_survivors = split_entities.then(|| entities.iter().map(|entity| entity.id.clone()).collect::<Vec<_>>());
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
        let page_matches = split_page_matches(split_survivors, resolved.as_ref());
        let entity_map_uri = record_map_use(&state.entity_maps, map_use, contributions, map_entities, page_matches, &uri).await?;

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

/// One query, decoded and admitted: what the protocol headers contributed to
/// it, the Entity Map it runs under, and the options its response is shaped by.
struct AdmittedQuery {
    /// The preferred natural language, from the query or the fallback header.
    language_filter: Option<LanguageFilter>,
    /// The `Prefer` header the response honours (clause 6.3.21).
    prefer: PreferHeader,
    /// The `Via` chain the request arrived with, replayed onto every forwarded
    /// copy so a forwarding loop is detected rather than followed.
    incoming_via: Option<Via>,
    /// The Entity Map the query runs under (clause 5.5.14).
    map_use: EntityMapUse,
    /// The representation, `sysAttrs` and `count` options resolved (clause 6.3.7).
    opts: EntityQueryParametersOptions,
}

/// Decode `protocol` into the query, hold the query to every rule that can be
/// decided before it runs, and resolve the Entity Map it runs under.
///
/// The extension headers add to the query before it is validated, so a value a
/// header supplied is held to the same rules as one the query string carried.
/// A branch that cannot federate is confined to the local scope first: clause
/// 5.5.15 makes a Snapshot "implicitly limited to a local scope ... overriding
/// setting a local scope to false".
///
/// # Errors
///
/// The classified failure of an extension header the query cannot take, of the
/// clause 5.7.2.4 validation, of resolving the Entity Map, of the Table 6.3.7-1
/// options, and of the clause 5.7.2.4 ordering rule.
async fn admit_request(
    state: &EntityRouteServices,
    target: &ContextTarget,
    context: &ContextSpec,
    protocol: NgsiLdProtocolHeaders,
    query: &mut EntityQueryParameters,
    binding: QueryBinding,
    instance: &HttpUri,
) -> Result<AdmittedQuery, ProblemResponse> {
    protocol.extensions().apply_to_entity_query(query).with_entity_instance(instance)?;
    // Clause 5.5.7: a request `@context` declaring a JSON-LD Scoped Context is
    // refused on the query path -- GET and POST alike, the latter after its body
    // `@context` has been merged into `context` -- before anything is expanded
    // against it.
    state.entity_query.admit_read_context(context).await.with_entity_instance(instance)?;
    if !target.permits_federation() {
        query.local = Some(true);
    }
    query.validate().with_entity_instance(instance)?;
    let referenced = protocol.entity_map.as_ref().map(NgsiLdEntityMap::as_inner);
    let map_use = resolve_query_map_use(&state.entity_maps, target, query, referenced, instance).await?;
    let opts = query.parse_options().with_entity_instance(instance)?;
    refuse_inadmissible_ordering(&state.forwarding, target, context, query, binding, EntityMapInUse::from(&map_use), instance).await?;
    Ok(AdmittedQuery {
        language_filter: query.lang.take().or(protocol.language_filter),
        prefer: protocol.prefer,
        incoming_via: protocol.incoming_via,
        map_use,
        opts,
    })
}

/// Refuse an ordering clause 5.7.2.4 does not admit.
///
/// The bullet raises `BadRequestData` for an `orderBy` on a query "not limited
/// to the local scope", which `andromeda_federation::ordering_admission` reads
/// as an answer a registered Context Source takes part in assembling. That is a
/// question for the Context Registry rather than for the parameters, so it is
/// asked here rather than during validation.
///
/// # Errors
///
/// The classified failure of reading the registrations that would take part,
/// and the clause 5.7.2.4 refusal itself when one of them does.
async fn refuse_inadmissible_ordering(
    forwarding: &FederationService,
    target: &ContextTarget,
    context: &ContextSpec,
    query: &EntityQueryParameters,
    binding: QueryBinding,
    entity_map: EntityMapInUse<'_>,
    instance: &HttpUri,
) -> Result<(), ProblemResponse> {
    let admission = forwarding
        .admit_ordering(
            target,
            context,
            OrderedRead {
                ordering: &query.ordering,
                requested_scope: query.execution_scope(),
                client_query: ClientQuery::EntityQuery(query),
                operation: binding.forwarding_operation(),
                entity_map,
            },
        )
        .await
        .with_entity_instance(instance)?;
    match admission {
        OrderingAdmission::Admitted => Ok(()),
        OrderingAdmission::Refused => Err(ParameterError::OrderingWithParticipatingSource).with_entity_instance(instance),
    }
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

/// How the map a `splitEntities` query ran under is pruned (clause 5.5.9.3).
///
/// `split_survivors` is `Some` only under `splitEntities`, carrying the entities
/// that survived the deferred filters over the merged result; the Linked
/// Entities the join reached join them, since clause 5.5.14 fixes them as part
/// of the same unitary request rather than as candidates a filter could shed.
/// A non-split query prunes per source.
fn split_page_matches(split_survivors: Option<Vec<Uri>>, resolved: Option<&ResolvedJoin<'_>>) -> PageMatches {
    match split_survivors {
        Some(mut survivors) => {
            if let Some(resolved) = resolved {
                survivors.extend(resolved.linked.keys().cloned()); // clone: the match set owns its identifiers
            }
            PageMatches::Merged(survivors)
        }
        None => PageMatches::PerSource,
    }
}
