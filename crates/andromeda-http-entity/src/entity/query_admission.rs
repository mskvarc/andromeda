//! Admitting a decoded entity query before it runs.
//!
//! Admission is everything that can be decided about a query before it reaches
//! the store: refusing a scoped read `@context`, confining a non-federating branch to the local scope, validating
//! it, resolving the Entity Map it runs under, parsing its response options,
//! expanding the filters the registrations are selected by, and holding it to
//! the clause 5.7.2.4 ordering rule. [`crate::entity::query::run`]
//! consumes the [`AdmittedQuery`] this produces.

use crate::{
    entity::{
        ContextSpec,
        EntityResultExt,
        NgsiLdProtocolHeaders,
        ProblemResponse,
        query_binding::QueryBinding,
        request_entity_map::{WrittenRequest, resolve_query_map_use},
    },
    state::EntityRouteServices,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::reuse::EntityMapUse;
use andromeda_extensions::advertisement::advertised_language_targeting;
use andromeda_federation::{
    FederationService,
    expanded_registration_filters::ExpandedRegistrationFilters,
    forwarded_query::client_query::ClientQuery,
    language_negotiation::NegotiatedLanguageTargeting,
    ordering_admission::OrderingAdmission,
    participating_read::ParticipatingRead,
    request::EntityMapInUse,
};
use andromeda_http_extraction::{pagination_policy::RequestPagination, prefer::PreferHeader};
use andromeda_http_headers::{language::LanguageFilter, ngsild_entity_map::NgsiLdEntityMap, via::Via};
use andromeda_jsonld::user_context_admission::AdmittedContext;
use andromeda_ngsi_ld_query::{
    entity_selection::EntitySelection,
    filter::reading::filter_reading::FilterReading,
    ordering::parameters::OrderingParameters,
    query_identity::{IdentifierScope, QueryIdentity, QueryIdentityError},
};
use andromeda_ngsi_ld_request::{
    error::ParameterError,
    pagination::page_pointers::PagePointers,
    query::entity::{
        admitted::AdmittedEntityQuery,
        operation::EntityQueryOperation,
        parameters::{EntityQueryParameters, EntityQueryParametersOptions},
    },
};
use axum::http::Uri as HttpUri;

/// One query, decoded and admitted: what the protocol headers contributed to
/// it, the Entity Map it runs under, and the options its response is shaped by.
pub(crate) struct AdmittedQuery {
    /// The natural-language preference the request's header states, which
    /// the response is rendered by when the query names no `lang`.
    pub(crate) fallback_language: Option<LanguageFilter>,
    /// The `Prefer` header the response honours (clause 6.3.21).
    pub(crate) prefer: PreferHeader,
    /// The `Via` chain the request arrived with, replayed onto every forwarded
    /// copy so a forwarding loop is detected rather than followed.
    pub(crate) incoming_via: Option<Via>,
    /// The Entity Map the query runs under (clause 5.5.14).
    pub(crate) map_use: EntityMapUse,
    /// The representation, `sysAttrs` and `count` options resolved (clause 6.3.7).
    pub(crate) opts: EntityQueryParametersOptions,
    /// The context source filter and geoquery a registration has to pass to
    /// take part (clauses 4.3.6.1, 4.9 and 5.10.2.4); neither when the query
    /// does not reach the Context Registry.
    pub(crate) registration_filters: ExpandedRegistrationFilters,
    /// What the query's target languages may be and how they select
    /// languageMap keys, locally and at every source it reaches, and the
    /// warning a fallback to tag equality carries.
    pub(crate) language: NegotiatedLanguageTargeting,
    /// The canonical identity of the query the request's URI writes, which its
    /// walk is paged under (clause 5.5.9.1); absent when the broker holds the
    /// query, whose walk is paged under the digest of the held query.
    pub(crate) walk_identity: Option<Vec<u8>>,
}

impl AdmittedQuery {
    /// The pagination of the request once its query is admitted: a walk over
    /// the request's URI is bound to the query as written (clause 5.5.9.1), so a
    /// followed cursor is checked against its identity and every pointer the
    /// page carries names it, while a walk over a held query keeps its digest.
    pub(crate) fn paged(&self, pagination: RequestPagination) -> RequestPagination {
        match self.walk_identity.as_deref() {
            Some(identity) => pagination.identified(identity),
            None => pagination,
        }
    }
}

/// Hold the query to every rule that can be decided before it runs, admitting
/// its `q` under the broker's complexity limits and target-language level, and
/// resolve the Entity Map it runs under.
///
/// A branch that cannot federate is confined to the local scope first: clause
/// 5.5.15 makes a Snapshot "implicitly limited to a local scope ... overriding
/// setting a local scope to false".
///
/// # Errors
///
/// The classified failure of the clause 5.7.2.4 validation, of the clause 5.5.6
/// `q` complexity limits and the broker's `q_target_languages` level, of
/// resolving the Entity Map, of the Table 6.3.7-1 options, of expanding the
/// registration filters, of the clause 5.7.2.4 ordering rule, and of the
/// levels of the sources the query reaches.
pub(crate) async fn admit_request(
    state: &EntityRouteServices,
    target: &ContextTarget,
    context: &AdmittedContext,
    protocol: NgsiLdProtocolHeaders,
    mut query: EntityQueryParameters,
    binding: &QueryBinding,
    instance: &HttpUri,
) -> Result<(AdmittedEntityQuery, AdmittedQuery), ProblemResponse> {
    if !target.permits_federation() {
        query.local = Some(true);
    }
    let service = &state.entity_query;
    let mut query = query
        .admit(EntityQueryOperation::Query, service.filter_admission())
        .with_entity_instance(instance)?;
    let registration_filters = registration_filters(state, context, &query, instance).await?;
    // The query as the client wrote it: an Entity Map narrows its identifiers
    // below, and split entities defer its filters, after this identity is taken.
    let written = service
        .compile_query_cached(&query, context, service.language_targeting())
        .await
        .with_entity_instance(instance)?;
    let walk_identity = walk_identity(&written.selection, registration_filters.csf(), binding.page_pointers()).with_entity_instance(instance)?;
    let referenced = protocol.entity_map.as_ref().map(NgsiLdEntityMap::as_inner);
    let map_use = resolve_query_map_use(
        &state.entity_maps,
        target,
        &mut query,
        WrittenRequest {
            selection: &written.selection,
            csf: registration_filters.csf(),
            context,
        },
        referenced,
        instance,
    )
    .await?;
    let opts = query.parse_options().with_entity_instance(instance)?;
    let read = ParticipatingRead {
        requested_scope: query.execution_scope(),
        client_query: ClientQuery::EntityQuery(&query),
        geo: registration_filters.geo(),
        operation: binding.forwarding_operation(),
        entity_map: EntityMapInUse::from(&map_use),
    };
    refuse_inadmissible_ordering(&state.forwarding, target, context, &query.ordering, &read, instance).await?;
    let language = state
        .forwarding
        .negotiate_language_targeting(target, context, &read, service.language_targeting(), advertised_language_targeting)
        .await
        .with_entity_instance(instance)?;
    let admitted = AdmittedQuery {
        fallback_language: protocol.language_filter,
        prefer: protocol.prefer,
        incoming_via: protocol.incoming_via,
        map_use,
        opts,
        registration_filters,
        language,
        walk_identity,
    };
    Ok((query, admitted))
}

/// The identity of the compiled query a walk over the request's URI is paged
/// under, with the identifiers kept: a cursor walks one result, and the
/// identifiers decide it.
///
/// # Errors
///
/// [`QueryIdentityError`] when the identity cannot be written as bytes.
fn walk_identity(selection: &EntitySelection, csf: Option<&FilterReading>, pointers: PagePointers) -> Result<Option<Vec<u8>>, QueryIdentityError> {
    match pointers {
        PagePointers::OverRequestUri => QueryIdentity::of(selection, csf, IdentifierScope::Kept).record().map(Some),
        PagePointers::OverPostedQuery | PagePointers::Absent => Ok(None),
    }
}

/// The query's context source filter and geoquery expanded under the
/// request's context, when the query reaches the Context Registry: clause
/// 5.10.2.4 matches the geoquery against the geometry each registration
/// declares, and clause 4.9 the filter against its Context Source Properties.
///
/// # Errors
///
/// The classified failure of compiling the geoquery or expanding the filter.
pub(crate) async fn registration_filters(
    state: &EntityRouteServices,
    context: &ContextSpec,
    query: &EntityQueryParameters,
    instance: &HttpUri,
) -> Result<ExpandedRegistrationFilters, ProblemResponse> {
    if query.is_local() {
        return Ok(ExpandedRegistrationFilters::default());
    }
    let service = &state.entity_query;
    ExpandedRegistrationFilters::expand(service.engine(), context, query.csf.as_ref(), query.geo.0.as_ref(), service.geo_admission())
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
