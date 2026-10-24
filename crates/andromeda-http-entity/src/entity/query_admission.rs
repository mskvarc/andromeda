//! Admitting a decoded entity query before it runs.
//!
//! Admission is everything that can be decided about a query before it reaches
//! the store: refusing a scoped read `@context`, confining a non-federating branch to the local scope, validating
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
use andromeda_extensions::advertisement::advertised_language_targeting;
use andromeda_federation::{
    FederationService,
    forwarded_query::client_query::ClientQuery,
    language_negotiation::NegotiatedLanguageTargeting,
    ordering_admission::OrderingAdmission,
    participating_read::ParticipatingRead,
    request::EntityMapInUse,
};
use andromeda_http_extraction::prefer::PreferHeader;
use andromeda_http_headers::{language::LanguageFilter, ngsild_entity_map::NgsiLdEntityMap, via::Via};
use andromeda_jsonld::{term_expansion::ContextualExpander, user_context_admission::AdmittedContext};
use andromeda_ngsi_ld_query::{
    geo::{compile::compile_geo_query, query::GeoQuery},
    ordering::parameters::OrderingParameters,
};
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
    /// What the query's target languages may be and how they select
    /// languageMap keys, locally and at every source it reaches, and the
    /// warning a fallback to tag equality carries.
    pub(crate) language: NegotiatedLanguageTargeting,
}

/// Hold the query to every rule that can be decided before it runs, and
/// resolve the Entity Map it runs under.
///
/// A branch that cannot federate is confined to the local scope first: clause
/// 5.5.15 makes a Snapshot "implicitly limited to a local scope ... overriding
/// setting a local scope to false".
///
/// # Errors
///
/// The classified failure of the clause 5.7.2.4 validation, of resolving the Entity Map, of the Table 6.3.7-1
/// options, of compiling the geoquery, of the clause 5.7.2.4 ordering rule, and
/// of admitting the `q` target languages under the broker's
/// `q_target_languages` level and the levels of the sources the query reaches.
pub(crate) async fn admit_request(
    state: &EntityRouteServices,
    target: &ContextTarget,
    context: &AdmittedContext,
    protocol: NgsiLdProtocolHeaders,
    query: &mut EntityQueryParameters,
    binding: QueryBinding,
    instance: &HttpUri,
) -> Result<AdmittedQuery, ProblemResponse> {
    if !target.permits_federation() {
        query.local = Some(true);
    }
    query.validate(EntityQueryOperation::Query).with_entity_instance(instance)?;
    let referenced = protocol.entity_map.as_ref().map(NgsiLdEntityMap::as_inner);
    let map_use = resolve_query_map_use(&state.entity_maps, target, query, referenced, instance).await?;
    let opts = query.parse_options().with_entity_instance(instance)?;
    let registry_geo = registry_geo_query(state, context, query, instance).await?;
    let service = &state.entity_query;
    // The broker refuses a target language its own `q_target_languages`
    // level does not admit before any source is asked.
    if let Some(q) = &query.q {
        service.language_targeting().admit(q).with_entity_instance(instance)?;
    }
    let read = ParticipatingRead {
        requested_scope: query.execution_scope(),
        client_query: ClientQuery::EntityQuery(query),
        geo: registry_geo.as_ref(),
        operation: binding.forwarding_operation(),
        entity_map: EntityMapInUse::from(&map_use),
    };
    refuse_inadmissible_ordering(&state.forwarding, target, context, &query.ordering, &read, instance).await?;
    let language = state
        .forwarding
        .negotiate_language_targeting(target, context, &read, service.language_targeting(), advertised_language_targeting)
        .await
        .with_entity_instance(instance)?;
    Ok(AdmittedQuery {
        language_filter: query.lang.take().or(protocol.language_filter),
        prefer: protocol.prefer,
        incoming_via: protocol.incoming_via,
        map_use,
        opts,
        registry_geo,
        language,
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
    ordering: &OrderingParameters,
    read: &ParticipatingRead<'_>,
    instance: &HttpUri,
) -> Result<(), ProblemResponse> {
    let admission = forwarding
        .admit_ordering(target, context, ordering, read)
        .await
        .with_entity_instance(instance)?;
    match admission {
        OrderingAdmission::Admitted => Ok(()),
        OrderingAdmission::Refused => Err(ParameterError::OrderingWithParticipatingSource).with_entity_instance(instance),
    }
}
