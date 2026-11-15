//! Admitting a decoded temporal query before it runs.
//!
//! Admission is everything that can be decided about a query before it reaches
//! the store: confining a non-federating branch to the local scope, expanding
//! the filters the registrations are selected by, deciding the element page,
//! resolving the Entity Map it runs under, validating it, and holding it to the
//! clause 5.7.4.4 ordering rule.

use crate::{
    state::TemporalRouteServices,
    temporal::{
        ContextSpec,
        NgsiLdProtocolHeaders,
        ProblemResponse,
        ResultInstanceExt,
        TemporalQueryParameters,
        query::{
            page::{PageSource, QueriedPage},
            request_entity_map::resolve_query_map,
            written_query::WrittenQuery,
        },
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::reuse::EntityMapUse;
use andromeda_extensions::advertisement::advertised_language_targeting;
use andromeda_federation::{
    FederationService,
    expanded_registration_filters::ExpandedRegistrationFilters,
    forwarded_query::client_query::ClientQuery,
    language_negotiation::NegotiatedLanguageTargeting,
    operation::ForwardingOperation,
    ordering_admission::OrderingAdmission,
    participating_read::ParticipatingRead,
    request::EntityMapInUse,
};
use andromeda_http_extraction::prefer::PreferHeader;
use andromeda_http_headers::via::Via;
use andromeda_jsonld::user_context_admission::AdmittedContext;
use andromeda_ngsi_ld_query::{
    ordering::parameters::OrderingParameters,
    query_identity::{IdentifierScope, QueryIdentity},
};
use andromeda_ngsi_ld_request::{
    error::ParameterError,
    pagination::page_pointers::PagePointers,
    query::temporal::{admitted::AdmittedTemporalRequest, operation::TemporalQueryOperation},
};
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
    /// The context source filter and geoquery a registration has to pass to
    /// take part (clauses 4.3.6.1, 4.9 and 5.10.2.4); neither when the query
    /// does not reach the Context Registry.
    pub(super) registration_filters: ExpandedRegistrationFilters,
    /// What the query's target languages may be and how they select
    /// languageMap keys, locally and at every source it reaches, and the
    /// warning a fallback to tag equality carries.
    pub(super) language: NegotiatedLanguageTargeting,
    /// The element page the query answers with (clause 6.3.10).
    pub(super) page: QueriedPage,
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

/// Hold the query to every rule that can be decided before it runs, admitting
/// its `q` under the broker's complexity limits and target-language level,
/// decide its element page and resolve the Entity Map it runs under.
///
/// A branch that cannot federate is confined to the local scope first: clause
/// 5.5.15 makes a Snapshot "implicitly limited to a local scope ... overriding
/// setting a local scope to false".
///
/// The page is decided before the Entity Map is resolved and before any
/// context source is contacted, so a pagination parameter the collection
/// cannot honour is refused first. A walk over the request's URI is paged
/// under the identity of the query as the client wrote it, so its page is
/// decided once that query compiles.
///
/// # Errors
///
/// The classified failure of expanding the registration filters, of the
/// pagination policy, of resolving the Entity Map, of the clause 5.7.4.4
/// validation, of the clause 5.5.6 `q` complexity limits and the broker's
/// `q_target_languages` level, of the clause 5.7.4.4 ordering rule, and of the
/// levels of the sources the query reaches.
pub(super) async fn admit_request(
    state: &TemporalRouteServices,
    target: &ContextTarget,
    context: &AdmittedContext,
    protocol: NgsiLdProtocolHeaders,
    mut params: TemporalQueryParameters,
    page: PageSource,
    instance: &Uri,
) -> Result<(AdmittedTemporalRequest, AdmittedQuery), ProblemResponse> {
    params.lang = params.lang.take().or(protocol.language_filter);
    if !target.permits_federation() {
        params.local = Some(true);
    }
    let service = &state.temporal;
    let registration_filters = registration_filters(state, context, &params, instance).await?;
    // clone: the query as written is read beside the request's own parameters, which an Entity Map narrows
    let written = WrittenQuery::new(service, context, params.clone(), instance);
    let page = match page {
        PageSource::Decided(page) => page,
        PageSource::OverRequestUri(pagination) => {
            let identity = QueryIdentity::of_temporal(written.compiled().await?, registration_filters.csf(), IdentifierScope::Kept)
                .record()
                .with_instance(instance)?;
            QueriedPage::requested(pagination.identified(&identity), &params, PagePointers::OverRequestUri).with_instance(instance)?
        }
    };
    let map_use = resolve_query_map(
        &state.entity_maps,
        target,
        &mut params,
        &written,
        registration_filters.csf(),
        protocol.entity_map.as_ref(),
        instance,
    )
    .await?;
    let params = params
        .admit(TemporalQueryOperation::Query, service.linked_retrieval(), service.filter_admission())
        .with_instance(instance)?;
    let read = ParticipatingRead {
        requested_scope: params.execution_scope(),
        client_query: ClientQuery::TemporalQuery(&params),
        geo: registration_filters.geo(),
        operation: ForwardingOperation::QueryTemporalEntity,
        entity_map: EntityMapInUse::from(&map_use),
    };
    refuse_inadmissible_ordering(&state.forwarding, target, context, &params.ordering, &read, instance).await?;
    let language = state
        .forwarding
        .negotiate_language_targeting(target, context, &read, service.language_targeting(), advertised_language_targeting)
        .await
        .with_instance(instance)?;
    let admitted = AdmittedQuery {
        prefer: protocol.prefer,
        incoming_via: protocol.incoming_via,
        map_use,
        registration_filters,
        language,
        page,
    };
    Ok((params, admitted))
}

/// The query's context source filter and geoquery expanded under the
/// request's context, when the query reaches the Context Registry: clause
/// 5.10.2.4 matches the geoquery against the geometry each registration
/// declares, and clause 4.9 the filter against its Context Source Properties.
///
/// # Errors
///
/// The classified failure of compiling the geoquery or expanding the filter.
async fn registration_filters(
    state: &TemporalRouteServices,
    context: &ContextSpec,
    params: &TemporalQueryParameters,
    instance: &Uri,
) -> Result<ExpandedRegistrationFilters, ProblemResponse> {
    if params.is_local() {
        return Ok(ExpandedRegistrationFilters::default());
    }
    let service = &state.temporal;
    ExpandedRegistrationFilters::expand(service.engine(), context, params.csf.as_ref(), params.geo.0.as_ref(), service.geo_admission())
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
