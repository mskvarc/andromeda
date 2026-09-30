//! The `EntityMap` a temporal query runs under (clauses 5.5.14 and 5.14.5).
//!
//! Resolving what the query runs under happens before any context source is
//! contacted, since the map fixes which of them are contacted at all; recording
//! what they contributed, and shedding the entities the prepared page found no
//! longer matching, happens once they have answered.

use crate::{
    error::TemporalHttpClassify,
    history_read::TemporalHistoryRead,
    temporal::{ProblemResponse, ResultInstanceExt, TemporalQueryParameters, TimeRel, local_map_entities::LocalHistoryEntities},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::{
    contribution::SourceContribution,
    fingerprint::{FingerprintedRequest, fingerprint_of},
    query_scoping::hold_selection_to_map,
    recording::{PageMatches, RecordingFailure},
    reuse::{EntityMapFlag, EntityMapUse},
    service::EntityMapService,
};
use andromeda_http_headers::ngsild_entity_map::NgsiLdEntityMap;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_temporal::{error::TemporalError, temporal_invariant::TemporalInvariant};
use axum::http::Uri;
use chrono::{DateTime, Utc};

/// Resolve the Entity Map the query runs under and hold the query to it.
///
/// Clause 5.5.14: a referenced live map fixes the entities the query considers
/// and the sources it contacts, and holds the query to the creating request's
/// filters (the temporal window excepted); an expired or unknown one yields no
/// inference, so a fresh map is created instead. The fingerprint is taken
/// before the query is narrowed. A map-scoped follow-up may omit the temporal
/// window, which then stands for all history up to a far-future horizon.
///
/// # Errors
///
/// The classified failure of fingerprinting the request or of reading the map
/// store, and [`TemporalError::Validation`] when the far-future horizon cannot
/// be parsed.
pub(crate) async fn resolve_query_map(
    entity_maps: &EntityMapService,
    target: &ContextTarget,
    params: &mut TemporalQueryParameters,
    referenced: Option<&NgsiLdEntityMap>,
    instance: &Uri,
) -> Result<EntityMapUse, ProblemResponse> {
    if !target.permits_federation() {
        return Ok(EntityMapUse::None);
    }
    let fingerprint = fingerprint_of(FingerprintedRequest::QueryTemporal(params)).with_instance(instance)?;
    let map_use = entity_maps
        .resolve_use(
            referenced.map(NgsiLdEntityMap::as_inner),
            EntityMapFlag::from_parameter(params.entity_map),
            &fingerprint,
            params.entity_map_lifetime.as_ref(),
        )
        .await
        .with_instance(instance)?;
    if let Some(map) = map_use.reused() {
        hold_selection_to_map(map, &mut params.id, &mut params.entity_selectors);
        if params.timerel.is_none() && params.time_at.is_none() {
            params.timerel = Some(TimeRel::Before);
            params.time_at = Some(
                DateTime::parse_from_rfc3339("9999-12-31T23:59:59Z")
                    .map(|instant| instant.with_timezone(&Utc))
                    .map_err(|source| TemporalError::from(TemporalInvariant::EntityMapHorizon(source)))
                    .with_instance(instance)?,
            );
        }
    }
    Ok(map_use)
}

/// Record the Entity Map the query ran under and return its location.
///
/// The recording itself is the same for every read that runs under a map and
/// belongs to the map policy; this is the temporal query's half of it: the
/// histories this broker still answers with, together with the Linked Entities
/// the join reached, which clause 5.5.14 counts as part of the same unitary
/// request, and the classification of the two vocabularies of failure.
///
/// # Errors
///
/// The classified failure of listing the matching identifiers or of writing to
/// the map store.
pub(crate) async fn record_query_map(
    entity_maps: &EntityMapService,
    read: &TemporalHistoryRead<'_>,
    map_use: EntityMapUse,
    contributions: Vec<SourceContribution>,
    linked_ids: Vec<Identifier>,
    instance: &Uri,
) -> Result<Option<Identifier>, ProblemResponse> {
    let local = LocalHistoryEntities::Queried {
        read,
        linked: &linked_ids,
        instance,
    };
    entity_maps
        // Temporal split pruning is not narrowed by the deferred filters here,
        // so each source's answer is recorded on its own terms (clause 5.7.4.4).
        .record_read(map_use, contributions, &local, local.page_filtering(), PageMatches::PerSource)
        .await
        .map_err(|failure| match failure {
            RecordingFailure::LocalEntities(problem) => problem,
            RecordingFailure::Store(error) => error.classify_for_temporal_http().with_instance(instance.clone()), // clone: the problem response owns the request URI it names
        })
}
