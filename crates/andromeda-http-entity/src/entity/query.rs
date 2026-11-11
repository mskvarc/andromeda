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
        EntityQuery,
        EntityResultExt,
        EntityTenantState,
        GeometrySelection,
        NgsiLdProtocolHeaders,
        NgsiLdRequestEnvelope,
        ProblemResponse,
        RequestProjection,
        ResolvedJoin,
        Response,
        StatusCode,
        query_admission::admit_request,
        query_binding::QueryBinding,
        query_forwarding::{ForwardedQueryResults, MergedEntities, QueryForwarding},
        query_local_page::local_versions,
        query_page::{QueryPage, requested_window},
        query_response::{QueryCollectionMetadata, query_response},
        query_split_filters::retain_matching,
        request_entity_map::{LocalEntitySource, record_map_use},
        with_linked_resolution_warnings,
    },
    join::JoinRequest,
    read_rendering::{EntityRepresentation, matched_entities, settle_representation},
    state::EntityRouteServices,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::{EntityQueryService, entity_versions::VersionRead};
use andromeda_entity_map::recording::PageMatches;
use andromeda_failure_classification::result_limit_failure::classify_result_budget_refusal;
use andromeda_federation::split_entities::SplitEntities;
use andromeda_http_context_registry::response_rendering::ResponseContextSource;
use andromeda_http_extraction::{api_root::ApiRoot, context_target, pagination_policy::RequestPagination, response_context_request::ResponseContextRequest};
use andromeda_http_response::unreadable_entity_warning::with_unreadable_entity_warnings;
use andromeda_jsonld::user_context_admission::{AdmittedContext, SubmittedContext, admit_user_context};
use andromeda_linked_retrieval::{narrowing::list::ListInForce, plan::JoinPlan, typed::TypedEntities};
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::entity_selection::ConsideredEntities;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_request::{geometry_media_type::require_geojson_for_geometry_property, query::entity::parameters::EntityQueryParameters};
use axum::http::{HeaderMap, Uri as HttpUri};
use futures::future::OptionFuture;
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// One "Query Entities" as its binding decoded it.
///
/// Everything here is what the operation needs and nothing is specific to the
/// binding that supplied it: the query string binding of clause 6.4.3.2 fills
/// [`EntityQueryRequest::query`] from the URL, the POST binding of clause
/// 6.23.3.1 fills it from the clause 5.2.23 body, and from there on the two are
/// the same request.
#[derive(TypedBuilder)]
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
    /// The `apiRoot` a wrapper the response `Link` names is served under.
    pub(crate) api_root: ApiRoot,
    /// The request's headers, replayed onto every forwarded copy so a
    /// registration's `contextSourceInfo` can pass one of them through
    /// (clause 6.3.19).
    pub(crate) headers: HeaderMap,
    /// The JSON-LD context the query's terms expand under (clause 6.3.5),
    /// admitted (clause 5.5.7).
    pub(crate) context: AdmittedContext,
    /// The query itself, in the raw form the binding decoded it to.
    pub(crate) query: EntityQueryParameters,
    /// Which binding decoded it, which decides how the query is forwarded and
    /// whether the response can point at its neighbouring pages.
    pub(crate) binding: QueryBinding,
    /// How the broker pages its collections (clause 5.5.9), and the
    /// `@context` the query is paged under.
    pub(crate) pagination: RequestPagination,
}

/// Query Entities through its query-string binding (clause 6.4.3.2).
pub(crate) async fn query(
    context_target::ExistingTargetedTenantState { services, target, snapshot: _ }: EntityTenantState,
    accept: AcceptFormat,
    NgsiLdRequestEnvelope { uri, incoming_via: _, headers }: NgsiLdRequestEnvelope,
    ResponseContextRequest { context, api_root }: ResponseContextRequest,
    protocol: NgsiLdProtocolHeaders,
    pagination: RequestPagination,
    EntityQuery(query): EntityQuery,
) -> Result<Response, ProblemResponse> {
    // Clause 5.5.7 addresses the request's `@context` on every operation that
    // expands one, so a query admits it before anything is read under it.
    let context = admit_user_context(services.entity_query.engine(), SubmittedContext::from(context))
        .await
        .with_entity_instance(&uri)?;
    run(EntityQueryRequest::builder()
        .services(services)
        .target(target)
        .accept(accept)
        .protocol(protocol)
        .uri(uri)
        .api_root(api_root)
        .headers(headers)
        .context(context)
        .query(query)
        .binding(QueryBinding::QueryString)
        .pagination(pagination)
        .build())
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
        api_root,
        headers,
        context,
        mut query,
        binding,
        pagination,
    } = request;
    Box::pin(async move {
        let service = &state.entity_query;
        let admitted = admit_request(&state, &target, &context, protocol, &mut query, binding, &uri).await?;
        let opts = &admitted.opts;
        // Clauses 5.7.2.4 and 6.3.4: refused before the store is read.
        require_geojson_for_geometry_property(query.geometry_property.as_ref(), accept.media_type()).with_entity_instance(&uri)?;

        let window = requested_window(&pagination, &query, binding.page_pointers()).with_entity_instance(&uri)?;

        let is_local = !target.permits_federation() || query.is_local();
        let split = state.forwarding.split_entities(query.split_entities);
        let split_entities = !is_local && split == SplitEntities::Split;

        // Clause 5.5.14: an EntityMap "fixes the Entities to be considered for
        // subsequent requests based on it", so a request left considering none
        // (its identifiers and the map's do not intersect) has no entity for
        // any context source to hold, and contacting them could only widen the
        // answer the map fixes.
        let considers_nothing = matches!(ConsideredEntities::of_request(query.id.as_deref()), ConsideredEntities::NoEntity);

        // Forwarding reads the query as the client wrote it, so it runs before
        // the split-entities filters are held back from the local query below.
        let forwarding = (!is_local && !considers_nothing).then(|| QueryForwarding {
            forwarding: &state.forwarding,
            target: &target,
            uri: &uri,
            query: &query,
            admitted: &admitted,
            split,
            context: &context,
            headers: &headers,
            delivery: binding.delivery(),
        });
        let forwarded = OptionFuture::from(forwarding.as_ref().map(QueryForwarding::run)).await.transpose()?;

        // Clause 5.7.2.4: with `splitEntities`, `q`, the geoquery and `scopeQ`
        // are decided on the merged entities rather than by any one source.
        let deferred = split_entities.then(|| query.defer_split_filters());
        let page = QueryPage::of(
            window,
            query.count,
            forwarded.as_ref(),
            binding.page_pointers(),
            pagination.context,
            pagination.pointed,
        );

        // Clause 4.21: the request's `pick`/`omit` (or `attrs`) govern the
        // merged entities, which hold every member their sources returned, and,
        // through the list in force, every Linked Entity the traversal reaches
        // (clause 5.7.2.5). Both read it off the compilation the query already
        // ran, which has to outlive the join that borrows its terms.
        let request_projection = RequestProjection::of(service, &query, &context).await.with_entity_instance(&uri)?;
        let projection = request_projection.projection();

        let targeting = admitted.language.targeting;
        let local = local_versions(service, &target, &query, &page, &context, targeting, &uri).await?;
        let merged = merge_contributions(local.versions, forwarded, request_projection.list(), &uri)?;
        let MergedEntities {
            entities: versions,
            warnings,
            contributions,
        } = retain_matching(service, &target, merged, deferred, &context, targeting.matching(), &uri).await?;
        // Clause 6.3.17: a query evaluated by tag equality because a reached
        // source lacks extended filtering says so.
        let warnings = warnings.into_iter().chain(admitted.language.warning).collect::<Vec<_>>();
        let warnings = with_unreadable_entity_warnings(warnings, &local.unreadable);
        let split_survivors = split_entities.then(|| split_match_set(&versions));
        let (mut versions, reach, total_count) = page.cut(versions, |version| version.state.id().as_str(), local.total, local.reach);
        let carried = ResolvedJoin::carried_by(&mut versions);
        let mut entities = compacted_page(service, &versions, opts.sys_attrs, &query, &context, &uri).await?;

        // The join resolves before the EntityMap is recorded so a minted map
        // can include the linked ids (clauses 5.5.14 and 5.7.2.4).
        let join = JoinRequest::of_query(&query, opts.sys_attrs, request_projection.list(), &carried);
        let mut resolved = ResolvedJoin::prepare(service, &target, &entities, &context, &join)
            .await
            .with_entity_instance(&uri)?;
        // Clause 6.3.17: a Linked Entity the join could not reach leaves the
        // body incomplete, and the warning header is what says so.
        let warnings = with_linked_resolution_warnings(warnings, ResolvedJoin::failures_of(resolved.as_ref()));

        let representation = EntityRepresentation::new(opts.render_mode, projection);
        settle_representation(service, &representation, &context, &mut entities, resolved.as_mut(), &uri).await?;
        let entities = entities;
        let resolved = resolved;
        let plan = resolved.as_ref().map(ResolvedJoin::plan);

        // Table 6.4.3.2-3 and Table 6.23.3.1-1: 201 when an EntityMap has been
        // (re)created.
        let status = if admitted.map_use.mints() { StatusCode::CREATED } else { StatusCode::OK };
        let map_entities = LocalEntitySource::queried(service, &target, &query, &context, targeting, plan.as_ref(), &uri);
        let page_matches = split_page_matches(split_survivors, plan.as_ref());
        let entity_map_uri = record_map_use(&state.entity_maps, admitted.map_use, contributions, map_entities, page_matches, &uri).await?;

        let answer = matched_entities(&entities, plan.as_ref(), &representation);
        let rendering = ResponseContextSource::new(service.engine(), &context, &state.context_hosting, &api_root)
            .start_rendering(accept.media_type(), admitted.prefer.rendering_preferences(), opts.render_mode)
            .await
            .with_entity_instance(&uri)?
            .with_geometry_selection(geometry_selection(&query));
        let rendered = answer.render(rendering, admitted.language_filter.as_ref(), &uri)?;
        Ok(query_response(
            rendered,
            QueryCollectionMetadata {
                status,
                warnings,
                total_count,
                entity_map_uri,
                reach,
                boundary: cursor_boundary(&entities),
            },
            &page,
            accept.media_type(),
        ))
    })
    .await
}

/// The clause 4.5.16.1 rule a `GeoJSON` answer to `query` selects its geometry
/// by: the `geometryProperty` it named and the `datasetId` values it narrowed
/// the instances to.
fn geometry_selection(query: &EntityQueryParameters) -> GeometrySelection<'_> {
    GeometrySelection::new(query.geometry_property.as_ref(), query.dataset_id.as_deref().unwrap_or_default())
}

/// The identifiers of the entities that survived the deferred filters over the
/// merged result of a `splitEntities` query.
///
/// Clause 5.5.9.3: under `splitEntities` those are the whole match set the
/// reused map is pruned against, so they are captured over the full result,
/// before the page window narrows it. A non-split page prunes per source.
fn split_match_set(versions: &[ReceivedState]) -> Vec<Identifier> {
    versions.iter().map(|version| version.state.id().clone()).collect() // clone: the map pruning owns the identifiers it keeps
}

/// The page of merged entities, compacted once under the request context:
/// what every holder contributed is written the same way.
///
/// # Errors
///
/// The classified failure of a compaction.
async fn compacted_page(
    service: &EntityQueryService,
    versions: &[ReceivedState],
    sys_attrs: SysAttrsInclusion,
    query: &EntityQueryParameters,
    context: &AdmittedContext,
    instance: &HttpUri,
) -> Result<Vec<Entity>, ProblemResponse> {
    let read = VersionRead {
        id_override: None,
        sys_attrs,
        dataset_ids: query.dataset_id.as_deref().unwrap_or_default(),
    };
    service.compact_versions(versions, &read, context).await.with_entity_instance(instance)
}

/// Fold what the context sources contributed onto the entities this broker
/// holds itself.
///
/// # Errors
///
/// `TooManyResults` when the merged collection exceeds the budget the sources
/// were walked under (clause 4.12).
fn merge_contributions(
    local_entities: Vec<ReceivedState>,
    forwarded: Option<ForwardedQueryResults>,
    list: ListInForce<'_>,
    instance: &HttpUri,
) -> Result<MergedEntities, ProblemResponse> {
    match forwarded {
        Some(results) => results
            .merge_onto(local_entities, list)
            .map_err(|exceeded| ProblemResponse::classified(classify_result_budget_refusal(&exceeded), instance.clone())),
        None => Ok(MergedEntities::local(local_entities)),
    }
}

/// The first and last identifiers of the page as displayed, from which a
/// clause 5.5.9.1 cursor pointer is computed; absent when the page is empty.
fn cursor_boundary(entities: &[Entity]) -> Option<(Identifier, Identifier)> {
    // clone: the pagination links own the identifiers, while the entities are
    // consumed by the rendering that follows.
    entities
        .first()
        .map(|entity| entity.id.clone())
        .zip(entities.last().map(|entity| entity.id.clone()))
}

/// How the map a `splitEntities` query ran under is pruned (clause 5.5.9.3).
///
/// `split_survivors` is `Some` only under `splitEntities`, carrying the entities
/// that survived the deferred filters over the merged result; the Linked
/// Entities the join reached join them, since clause 5.5.14 fixes them as part
/// of the same unitary request rather than as candidates a filter could shed.
/// A non-split query prunes per source.
fn split_page_matches(split_survivors: Option<Vec<Identifier>>, plan: Option<&JoinPlan<'_, TypedEntities>>) -> PageMatches {
    match split_survivors {
        Some(mut survivors) => {
            if let Some(plan) = plan {
                survivors.extend(plan.linked_ids().into_iter().cloned()); // clone: the match set owns its identifiers
            }
            PageMatches::Merged(survivors)
        }
        None => PageMatches::PerSource,
    }
}
