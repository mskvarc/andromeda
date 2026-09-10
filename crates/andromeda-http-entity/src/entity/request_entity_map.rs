//! The `EntityMap` a distributed read runs under (clause 5.5.14).
//!
//! A read may reference a map and may ask for one. Resolving what it runs
//! under happens before any source is contacted, since the map fixes which
//! sources are contacted at all; recording what those sources contributed
//! happens after they have answered. The two reads that do this — Query
//! Entities and Retrieve Entity — differ only in how the entities this broker
//! holds itself are listed, which is why that half alone is a parameter here.

use crate::{
    error::{EntityHttpClassify, EntityResultExt},
    join::ResolvedJoin,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::EntityQueryService;
use andromeda_entity_map::{
    contribution::SourceContribution,
    fingerprint::{FingerprintedRequest, fingerprint_of},
    recording::{LocalMapEntities, MapListing, PageFiltering, RecordingFailure},
    reuse::{EntityMapFlag, EntityMapUse},
    service::EntityMapService,
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::pagination::{CountPolicy, Pagination};
use andromeda_ngsi_ld_request::{entity::GetEntityParams, query::entity::parameters::EntityQueryParameters};
use axum::http::Uri as HttpUri;

/// Where the entities a map attributes to this broker come from.
///
/// Listing them is not free for a query — it costs a second pass over the
/// store — so the source is described here and read only when the recording
/// asks for it.
#[derive(Clone, Copy)]
pub(crate) enum LocalEntitySource<'a> {
    /// The single entity a retrieval held locally, if it held one.
    Retrieved(Option<&'a Uri>),
    /// Every entity a query matched locally, which is the whole result and not
    /// merely the page (clause 5.5.14).
    Queried {
        /// The query policy that lists the matching identifiers.
        service: &'a EntityQueryService,
        /// Live or snapshot branch the query read.
        target: &'a ContextTarget,
        /// The query, as it ran against the store.
        query: &'a EntityQueryParameters,
        /// The request's JSON-LD context.
        context: &'a ContextSpec,
        /// The resolved clause 4.5.23 join, whose Linked Entities belong to the
        /// same unitary request.
        join: Option<&'a ResolvedJoin<'a>>,
        /// The request URI a failed listing names.
        instance: &'a HttpUri,
    },
}

impl LocalEntitySource<'_> {
    /// Whether preparing this read's page applied the request's filters to the
    /// entities of the map it ran under (clause 4.3.6.7).
    ///
    /// A query applies its query, geoquery, scope query and attribute filters
    /// to an entity before it joins a result page; a retrieval applies none of
    /// them, reading the map only to learn which context sources hold the one
    /// entity it asked for.
    pub(crate) const fn page_filtering(&self) -> PageFiltering {
        match self {
            LocalEntitySource::Retrieved(_) => PageFiltering::NotApplied,
            LocalEntitySource::Queried { .. } => PageFiltering::Applied,
        }
    }
}

impl LocalMapEntities for LocalEntitySource<'_> {
    type Error = ProblemResponse;

    async fn identifiers(&self, listing: MapListing) -> Result<Vec<Uri>, ProblemResponse> {
        match *self {
            LocalEntitySource::Retrieved(entity_id) => Ok(entity_id.cloned().into_iter().collect()), // clone: the map owns its identifiers
            LocalEntitySource::Queried {
                service,
                target,
                query,
                context,
                join,
                instance,
            } => {
                let mut compiled = service.compile_query_public(query, context).await.with_entity_instance(instance)?;
                // Clause 5.5.14: the map fixes every entity the request matched,
                // so the identifier pass ignores the window the client asked for
                // and runs over the whole result.
                compiled.pagination = Pagination {
                    limit: u32::MAX,
                    offset: 0,
                    count: CountPolicy::Omit,
                };
                match listing {
                    MapListing::Minted => {}
                    // Clause 4.3.6.7 rechecks the filters of the request the map
                    // was created for, and clause 5.5.14 lets a follow-up name
                    // its own identifiers: a request asking about one entity of
                    // the map has not checked the others, so it must not shed
                    // them.
                    MapListing::Rechecked => compiled.selection.without_identifier_narrowing(),
                }
                let compiled = compiled;
                let mut identifiers = service.query_ids(target, &compiled).await.with_entity_instance(instance)?;
                // Clause 5.5.14: the Linked Entities the join resolved belong to
                // the same unitary request as the entities that named them.
                if let Some(join) = join {
                    identifiers.extend(join.linked.keys().cloned()); // clone: the map owns its identifiers
                }
                identifiers.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
                identifiers.dedup();
                Ok(identifiers)
            }
        }
    }
}

/// Resolve the map an entity query runs under, and hold the query to it.
///
/// Clause 5.5.14: a referenced live map fixes the entities the query considers
/// and the sources it contacts, and holds the query to the creating request's
/// filters; an expired or unknown one yields no inference, so a fresh map is
/// created instead. The fingerprint is taken before the query is narrowed, so
/// that a follow-up under the map is compared against the request as written.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when the request cannot be fingerprinted or
/// the map store cannot be read.
pub(crate) async fn resolve_query_map_use(
    maps: &EntityMapService,
    target: &ContextTarget,
    query: &mut EntityQueryParameters,
    referenced: Option<&Uri>,
    instance: &HttpUri,
) -> Result<EntityMapUse, ProblemResponse> {
    if !target.permits_federation() {
        return Ok(EntityMapUse::None);
    }
    let fingerprint = fingerprint_of(FingerprintedRequest::QueryEntities(query)).with_entity_instance(instance)?;
    let map_use = maps
        .resolve_use(
            referenced,
            EntityMapFlag::from_parameter(query.entity_map),
            &fingerprint,
            query.entity_map_lifetime.as_ref(),
        )
        .await
        .with_entity_instance(instance)?;
    if let Some(map) = map_use.reused() {
        query.id = Some(EntityMapService::scope_query_ids(map, query.id.as_deref()));
    }
    Ok(map_use)
}

/// Resolve the map a single-entity retrieval runs under.
///
/// Clause 5.7.1.4: a referenced live map fixes the registrations the retrieval
/// contacts; an expired or unknown one yields no inference, so a fresh map is
/// created instead. A local retrieval contacts no source and uses no map.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when the request cannot be fingerprinted or
/// the map store cannot be read.
pub(crate) async fn resolve_retrieval_map_use(
    maps: &EntityMapService,
    target: &ContextTarget,
    params: &GetEntityParams,
    referenced: Option<&Uri>,
    instance: &HttpUri,
) -> Result<EntityMapUse, ProblemResponse> {
    if !target.permits_federation() || params.local.unwrap_or(false) {
        return Ok(EntityMapUse::None);
    }
    let fingerprint = fingerprint_of(FingerprintedRequest::RetrieveEntity(params)).with_entity_instance(instance)?;
    maps.resolve_use(
        referenced,
        EntityMapFlag::from_parameter(params.entity_map),
        &fingerprint,
        params.entity_map_lifetime.as_ref(),
    )
    .await
    .with_entity_instance(instance)
}

/// Record what the read's sources contributed to the map it ran under, and
/// answer with the map's location (Table 6.5.3.1-1 `entityMap`).
///
/// The recording itself is the same for every read that runs under a map and
/// belongs to the map policy; this is the entity binding's half of it: where
/// the entities this broker holds come from, and how the two vocabularies of
/// failure are classified for an HTTP response.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when the entities of a minted map cannot be
/// listed or the map store cannot be written.
pub(crate) async fn record_map_use(
    maps: &EntityMapService,
    map_use: EntityMapUse,
    contributions: Vec<SourceContribution>,
    local: LocalEntitySource<'_>,
    instance: &HttpUri,
) -> Result<Option<Uri>, ProblemResponse> {
    maps.record_read(map_use, contributions, &local, local.page_filtering())
        .await
        .map_err(|failure| match failure {
            RecordingFailure::LocalEntities(problem) => problem,
            RecordingFailure::Store(error) => error.classify_for_entity_http().with_instance(instance.clone()), // clone: the problem response owns the request URI it names
        })
}
