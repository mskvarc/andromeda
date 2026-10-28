//! Admitting a decoded temporal query before it runs.
//!
//! Admission is everything that can be decided about a query before it reaches
//! the store: resolving the Entity Map it runs under, confining a non-federating
//! branch to the local scope, validating it, compiling the geoquery the registrations are selected by, and holding it
//! to the clause 5.7.4.4 ordering rule.

use crate::{
    state::TemporalRouteServices,
    temporal::{ContextSpec, NgsiLdProtocolHeaders, ProblemResponse, ResultInstanceExt, TemporalQueryParameters, query::request_entity_map::resolve_query_map},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::reuse::EntityMapUse;
use andromeda_extensions::advertisement::advertised_language_targeting;
use andromeda_federation::{
    FederationService,
    forwarded_query::client_query::ClientQuery,
    language_negotiation::NegotiatedLanguageTargeting,
    operation::ForwardingOperation,
    ordering_admission::OrderingAdmission,
    participating_read::ParticipatingRead,
    request::EntityMapInUse,
};
use andromeda_http_extraction::prefer::PreferHeader;
use andromeda_http_headers::via::Via;
use andromeda_jsonld::{term_expansion::ContextualExpander, user_context_admission::AdmittedContext};
use andromeda_ngsi_ld_query::{
    geo::{compile::compile_geo_query, query::GeoQuery},
    ordering::parameters::OrderingParameters,
};
use andromeda_ngsi_ld_request::{error::ParameterError, query::temporal::operation::TemporalQueryOperation};
use andromeda_temporal::query_reading::QueryReading;
use axum::http::Uri;

/// One temporal query, decoded and admitted: what the protocol headers
/// contributed to it, and the Entity Map it runs under.
pub(super) struct AdmittedQuery {
    /// The `Prefer` header the response honours (clause 6.3.21).
    pub(super) prefer: PreferHeader,
    /// The `Via` chain the request arrived with, replayed onto every forwarded
    /// copy so a forwarding loop is detected rather than followed.
    pub(super) incoming_via: Option<Via>,
    /// The Entity Map the query runs under (clause 5.5.14).
    pub(super) map_use: EntityMapUse,
    /// The geoquery a registration's declared geometry has to admit for the
    /// registration to take part (clauses 4.3.6.1 and 5.10.2.4); `None` when
    /// the query has none or does not reach the Context Registry.
    pub(super) registry_geo: Option<GeoQuery>,
    /// What the query's target languages may be and how they select
    /// languageMap keys, locally and at every source it reaches, and the
    /// warning a fallback to tag equality carries.
    pub(super) language: NegotiatedLanguageTargeting,
}

impl AdmittedQuery {
    /// How the query's text is read under `context`.
    pub(super) const fn reading<'a>(&self, context: &'a ContextSpec) -> QueryReading<'a> {
        QueryReading {
            context,
            language_targeting: self.language.targeting,
        }
    }
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
/// The classified failure of resolving the Entity Map, of the clause 5.7.4.4
/// validation, of compiling the geoquery, of the clause 5.7.4.4 ordering rule,
/// of the clause 5.5.6 `q` complexity limits, and of admitting the `q` target
/// languages under the broker's
/// `q_target_languages` level and the levels of the sources the query reaches.
pub(super) async fn admit_request(
    state: &TemporalRouteServices,
    target: &ContextTarget,
    context: &AdmittedContext,
    protocol: NgsiLdProtocolHeaders,
    params: &mut TemporalQueryParameters,
    instance: &Uri,
) -> Result<AdmittedQuery, ProblemResponse> {
    params.lang = params.lang.take().or(protocol.language_filter);
    let map_use = resolve_query_map(&state.entity_maps, target, params, protocol.entity_map.as_ref(), instance).await?;
    if !target.permits_federation() {
        params.local = Some(true);
    }
    params
        .validate(TemporalQueryOperation::Query, state.temporal.linked_retrieval())
        .with_instance(instance)?;
    let registry_geo = registry_geo_query(state, context, params, instance).await?;
    let service = &state.temporal;
    // The broker refuses a filter too complex for it to resolve, and a target
    // language its own `q_target_languages` level does not admit, before any
    // source is asked.
    if let Some(q) = &params.q {
        service.filter_complexity().admit(q).with_instance(instance)?;
        service.language_targeting().admit(q).with_instance(instance)?;
    }
    let read = ParticipatingRead {
        requested_scope: params.execution_scope(),
        client_query: ClientQuery::TemporalQuery(params),
        geo: registry_geo.as_ref(),
        operation: ForwardingOperation::QueryTemporalEntity,
        entity_map: EntityMapInUse::from(&map_use),
    };
    refuse_inadmissible_ordering(&state.forwarding, target, context, &params.ordering, &read, instance).await?;
    let language = state
        .forwarding
        .negotiate_language_targeting(target, context, &read, service.language_targeting(), advertised_language_targeting)
        .await
        .with_instance(instance)?;
    Ok(AdmittedQuery {
        prefer: protocol.prefer,
        incoming_via: protocol.incoming_via,
        map_use,
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
    state: &TemporalRouteServices,
    context: &ContextSpec,
    params: &TemporalQueryParameters,
    instance: &Uri,
) -> Result<Option<GeoQuery>, ProblemResponse> {
    if params.is_local() {
        return Ok(None);
    }
    let service = &state.temporal;
    let prepared_context = service.engine().prepare_term_context(context);
    compile_geo_query(
        params.geo.0.as_ref(),
        &ContextualExpander::new(service.engine(), &prepared_context),
        service.geo_admission(),
    )
    .await
    .with_instance(instance)
}

/// Refuse an ordering clause 5.7.4.4 does not admit.
///
/// The bullet raises `BadRequestData` for an `orderBy` on a query "not limited
/// to the local scope", which `andromeda_federation::ordering_admission` reads
/// as an answer a registered Context Source takes part in assembling. That is a
/// question for the Context Registry rather than for the parameters, so it is
/// asked here rather than during validation. Which member may order a temporal
/// query is a separate rule the parameters do settle, and validation keeps it.
///
/// # Errors
///
/// The classified failure of reading the registrations that would take part,
/// and the clause 5.7.4.4 refusal itself when one of them does.
async fn refuse_inadmissible_ordering(
    forwarding: &FederationService,
    target: &ContextTarget,
    context: &ContextSpec,
    ordering: &OrderingParameters,
    read: &ParticipatingRead<'_>,
    instance: &Uri,
) -> Result<(), ProblemResponse> {
    let admission = forwarding.admit_ordering(target, context, ordering, read).await.with_instance(instance)?;
    match admission {
        OrderingAdmission::Admitted => Ok(()),
        OrderingAdmission::Refused => Err(ParameterError::OrderingWithParticipatingSource).with_instance(instance),
    }
}
