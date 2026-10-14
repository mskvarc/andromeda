//! Admitting a decoded entity query before it runs.
//!
//! Admission is everything that can be decided about a query before it reaches
//! the store: folding the extension headers into it, refusing a scoped read
//! `@context`, confining a non-federating branch to the local scope, validating
//! it, resolving the Entity Map it runs under, parsing its response options,
//! compiling the geoquery the registrations are selected by, and holding it to
//! the clause 5.7.2.4 ordering rule. [`crate::entity::query::run`]
//! consumes the [`AdmittedQuery`] this produces.

use crate::{
    entity::{ContextSpec, EntityResultExt, NgsiLdProtocolHeaders, ProblemResponse, query_binding::QueryBinding, request_entity_map::resolve_query_map_use},
    state::EntityRouteServices,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::reuse::EntityMapUse;
use andromeda_federation::{
    FederationService,
    forwarded_query::client_query::ClientQuery,
    ordering_admission::{OrderedRead, OrderingAdmission},
    request::EntityMapInUse,
};
use andromeda_http_extraction::prefer::PreferHeader;
use andromeda_http_headers::{language::LanguageFilter, ngsild_entity_map::NgsiLdEntityMap, via::Via};
use andromeda_jsonld::{term_expansion::ContextualExpander, user_context_admission::AdmittedContext};
use andromeda_ngsi_ld_query::geo::{compile::compile_geo_query, query::GeoQuery};
use andromeda_ngsi_ld_request::{
    error::ParameterError,
    query::entity::{
        operation::EntityQueryOperation,
        parameters::{EntityQueryParameters, EntityQueryParametersOptions},
    },
};
use axum::http::Uri as HttpUri;

/// One query, decoded and admitted: what the protocol headers contributed to
/// it, the Entity Map it runs under, and the options its response is shaped by.
pub(crate) struct AdmittedQuery {
    /// The preferred natural language, from the query or the fallback header.
    pub(crate) language_filter: Option<LanguageFilter>,
    /// The `Prefer` header the response honours (clause 6.3.21).
    pub(crate) prefer: PreferHeader,
    /// The `Via` chain the request arrived with, replayed onto every forwarded
    /// copy so a forwarding loop is detected rather than followed.
    pub(crate) incoming_via: Option<Via>,
    /// The Entity Map the query runs under (clause 5.5.14).
    pub(crate) map_use: EntityMapUse,
    /// The representation, `sysAttrs` and `count` options resolved (clause 6.3.7).
    pub(crate) opts: EntityQueryParametersOptions,
    /// The geoquery a registration's declared geometry has to admit for the
    /// registration to take part (clauses 4.3.6.1 and 5.10.2.4); `None` when
    /// the query has none or does not reach the Context Registry.
    pub(crate) registry_geo: Option<GeoQuery>,
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
/// options, of compiling the geoquery, and of the clause 5.7.2.4 ordering rule.
pub(crate) async fn admit_request(
    state: &EntityRouteServices,
    target: &ContextTarget,
    context: &AdmittedContext,
    protocol: NgsiLdProtocolHeaders,
    query: &mut EntityQueryParameters,
    binding: QueryBinding,
    instance: &HttpUri,
) -> Result<AdmittedQuery, ProblemResponse> {
    protocol.extensions().apply_to_entity_query(query).with_entity_instance(instance)?;
    if !target.permits_federation() {
        query.local = Some(true);
    }
    query.validate(EntityQueryOperation::Query).with_entity_instance(instance)?;
    let referenced = protocol.entity_map.as_ref().map(NgsiLdEntityMap::as_inner);
    let map_use = resolve_query_map_use(&state.entity_maps, target, query, referenced, instance).await?;
    let opts = query.parse_options().with_entity_instance(instance)?;
    let registry_geo = registry_geo_query(state, context, query, instance).await?;
    let read = RegistryRead {
        query,
        binding,
        geo: registry_geo.as_ref(),
        entity_map: EntityMapInUse::from(&map_use),
    };
    refuse_inadmissible_ordering(&state.forwarding, target, context, read, instance).await?;
    Ok(AdmittedQuery {
        language_filter: query.lang.take().or(protocol.language_filter),
        prefer: protocol.prefer,
        incoming_via: protocol.incoming_via,
        map_use,
        opts,
        registry_geo,
    })
}

/// The query's geoquery compiled under the request's context, when the query
/// reaches the Context Registry: clause 5.10.2.4 matches it against the
/// geometry each registration declares.
///
/// # Errors
///
/// The classified failure of compiling the geoquery.
async fn registry_geo_query(
    state: &EntityRouteServices,
    context: &ContextSpec,
    query: &EntityQueryParameters,
    instance: &HttpUri,
) -> Result<Option<GeoQuery>, ProblemResponse> {
    if query.is_local() {
        return Ok(None);
    }
    let service = &state.entity_query;
    let prepared_context = service.engine().prepare_term_context(context);
    compile_geo_query(
        query.geo.0.as_ref(),
        &ContextualExpander::new(service.engine(), &prepared_context),
        service.geo_admission(),
    )
    .await
    .with_entity_instance(instance)
}

/// What the ordering rule asks the Context Registry about one query.
struct RegistryRead<'a> {
    /// The query as admitted so far.
    query: &'a EntityQueryParameters,
    /// The binding the query arrived on, which fixes the operation a
    /// registration has to advertise.
    binding: QueryBinding,
    /// The compiled geoquery a registration's geometry has to admit.
    geo: Option<&'a GeoQuery>,
    /// The Entity Map the query runs under.
    entity_map: EntityMapInUse<'a>,
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
    read: RegistryRead<'_>,
    instance: &HttpUri,
) -> Result<(), ProblemResponse> {
    let RegistryRead {
        query,
        binding,
        geo,
        entity_map,
    } = read;
    let admission = forwarding
        .admit_ordering(
            target,
            context,
            OrderedRead {
                ordering: &query.ordering,
                requested_scope: query.execution_scope(),
                client_query: ClientQuery::EntityQuery(query),
                geo,
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
