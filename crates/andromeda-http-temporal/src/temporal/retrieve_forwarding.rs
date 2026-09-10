//! The distributed half of a single-entity temporal retrieval (clause
//! 5.7.3.4): the Entity Map the retrieval runs under, the fan-out to every
//! matching registration, and the merge of what they returned onto the history
//! this broker holds itself.
//!
//! The response the assembled history is rendered into is decided by
//! `crate::temporal::retrieve`; this module decides only what there is to
//! render, which is the half that changes when a registration, a map or a
//! source's answer changes.

use crate::{
    error::{TemporalHttpClassify, temporal_problem},
    temporal::{
        ContextSpec,
        CsrModeFilter,
        ForwardingOperation,
        ProblemResponse,
        ResultInstanceExt,
        TemporalQueryParameters,
        Uri,
        Value,
        collect_temporal_warnings,
        local_map_entities::LocalHistoryEntities,
        merge_temporal_retrieve_results,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::{
    contribution::SourceContribution,
    fingerprint::{FingerprintedRequest, fingerprint_of},
    recording::RecordingFailure,
    reuse::{EntityMapFlag, EntityMapUse},
    service::EntityMapService,
};
use andromeda_federation::{
    FederationService,
    entity_result::temporal_contributions_of,
    forwarded_query::ClientQuery,
    outcome::{ForwardReadOutcome, MatchedSources},
    request::{EntityMapInUse, ForwardRetrieveRequest, TriggeringRequest},
    temporal_last_n::truncate_to_last_n,
};
use andromeda_http_headers::{ngsild_entity_map::NgsiLdEntityMap, ngsild_warning::NgsiLdWarning};
use andromeda_http_response::declined_representation::single_source_declined;
use andromeda_temporal::{
    error::TemporalError,
    linked::candidates::TemporalLinkedCandidate,
    service::{TemporalRange, TemporalReadResult},
};
use axum::http::Uri as RequestUri;
use serde_json::json;
use tracing::warn;

/// What a temporal retrieval assembled before its representation is rendered.
pub(crate) struct RetrievedHistory {
    /// The temporal entity the response carries.
    pub(crate) value: Value,
    /// Range metadata of the local read, present when it truncated the history.
    pub(crate) range: Option<TemporalRange>,
    /// The clause 6.3.17 warnings the forwarding produced.
    pub(crate) warnings: Vec<NgsiLdWarning>,
    /// The Linked Entities the local representation names, read off it before
    /// any format projection narrowed it (clause 4.5.23).
    pub(crate) linked_candidates: Vec<TemporalLinkedCandidate>,
    /// The Entity Map the retrieval ran under, when one applies (clause 5.5.14).
    pub(crate) entity_map_uri: Option<Uri>,
}

/// Resolve the Entity Map a distributed retrieval runs under.
///
/// Clause 5.7.3.4: a referenced live map fixes the registrations the retrieval
/// contacts; an expired or unknown one yields no inference, so a fresh map is
/// created instead.
///
/// # Errors
///
/// The classified failure of fingerprinting the request or of reading the map
/// store, including the clause 5.5.14 refusal of a map created by a request
/// with different parameters.
pub(crate) async fn resolve_retrieve_map(
    entity_maps: &EntityMapService,
    params: &TemporalQueryParameters,
    referenced: Option<&NgsiLdEntityMap>,
    instance: &RequestUri,
) -> Result<EntityMapUse, ProblemResponse> {
    let fingerprint = fingerprint_of(FingerprintedRequest::RetrieveTemporal(params)).with_instance(instance)?;
    entity_maps
        .resolve_use(
            referenced.map(NgsiLdEntityMap::as_inner),
            EntityMapFlag::from_parameter(params.entity_map),
            &fingerprint,
            params.entity_map_lifetime.as_ref(),
        )
        .await
        .with_instance(instance)
}

/// The distributed half of a single-entity temporal retrieval: how it reaches
/// the registered sources, and the Entity Map that fixes which of them are
/// contacted and records what they returned.
pub(crate) struct RetrievalSources<'a> {
    /// The service that contacts every matching registration.
    pub(crate) forwarding: &'a FederationService,
    /// The store the used or minted Entity Map is written to (clause 5.5.14).
    pub(crate) entity_maps: &'a EntityMapService,
    /// The triggering request every forwarded copy replays, so a forwarding
    /// loop is detected rather than followed (clause 6.3.18).
    pub(crate) triggering: TriggeringRequest<'a>,
    /// The Entity Map the retrieval runs under: a referenced live map fixes
    /// the registrations contacted, a minted one is created for this request
    /// (clause 5.7.3.4).
    pub(crate) map_use: EntityMapUse,
}

/// Retrieve the entity's history from every matching source and this broker.
///
/// `sources` carries the distributed machinery; `target`, `id`, `params` and
/// `context` name the history being retrieved.
///
/// # Errors
///
/// The Table 6.4.3.2-3 `NotImplemented` answer when the one source holding all
/// the data declined the requested representation, the classified failure of
/// the local read or of the map store, and `ResourceNotFound` when the entity
/// is held neither locally nor by any matching registration (clause 5.7.3.4).
pub(crate) async fn distributed_retrieval(
    sources: RetrievalSources<'_>,
    target: &ContextTarget,
    id: &Uri,
    params: &TemporalQueryParameters,
    context: &ContextSpec,
    local_result: Result<TemporalReadResult, TemporalError>,
    instance: &RequestUri,
) -> Result<RetrievedHistory, ProblemResponse> {
    let RetrievalSources {
        forwarding,
        entity_maps,
        triggering,
        map_use,
    } = sources;
    let forwarded = forwarding
        .forward_temporal_retrieve(
            target,
            ForwardRetrieveRequest {
                entity_id: id,
                exclude_registration: None,
                client_query: ClientQuery::Temporal(params),
                context,
                operation: ForwardingOperation::RetrieveTemporalEntity,
                triggering,
                mode_filter: CsrModeFilter::All,
                entity_map: EntityMapInUse::from(&map_use),
            },
        )
        .await
        .unwrap_or_else(|error| {
            warn!(%error, "context source forwarding failed during the temporal retrieval");
            ForwardReadOutcome::empty()
        });
    if let Some(status) = forwarded.sources.single_source_status {
        return Err(single_source_declined(status, instance));
    }
    let warnings = collect_temporal_warnings(&forwarded.results);
    // Capture which registration returned which entity, and with which map,
    // before the merge (clauses 5.2.39 and 5.5.14).
    let contributions: Vec<SourceContribution> = temporal_contributions_of(&forwarded.results).collect();

    // Clauses 5.6.12.4 and 5.7.3.4: a local not-found beside a matching
    // registration is nothing held here, not an error.
    let (local_entity, range, linked_candidates) = match local_result {
        Ok(result) => (Some(result.value), result.range, result.linked_candidates),
        Err(ref error) if error.is_not_found() => (None, None, Vec::new()),
        Err(error) => return Err(temporal_problem(&error, instance)),
    };

    let had_remote_history = forwarded.results.iter().any(|result| !result.entities.is_empty());
    let local_id = local_entity.is_some().then(|| id.clone()); // clone: the map owns the entity identifiers it names
    let merged = merge_temporal_retrieve_results(local_entity, forwarded.results);
    let entity_map_uri = record_retrieve_map(entity_maps, map_use, contributions, local_id, instance).await?;

    let value = match merged {
        Some(mut value) => {
            // Clause 5.7.3.4: `lastN` bounds the instances of the merged
            // history, so it is applied once more over the aggregation.
            if let (true, Some(last_n)) = (had_remote_history, params.last_n) {
                truncate_to_last_n(&mut value, last_n, params.timeproperty_or_default());
            }
            value
        }
        // Clause 5.7.3.4: ResourceNotFound only when nothing is held locally
        // and no registration matches; a matching registration that returned
        // nothing leaves the entity known by its registered identity alone.
        None => match registered_identity(id, &forwarded.sources) {
            Some(identity) => identity,
            None => return Err(temporal_problem(&TemporalError::not_found(id.to_string()), instance).with_warnings(warnings)),
        },
    };
    Ok(RetrievedHistory {
        value,
        range,
        warnings,
        linked_candidates,
        entity_map_uri,
    })
}

/// Record the Entity Map the retrieval ran under and return its location.
///
/// Table 6.19.3.1-1 `entityMap`: the map the retrieval used is returned,
/// created now when none was in use (clause 5.7.3.4), naming this broker and
/// every registration that returned history; a reused map instead records the
/// maps its own sources answered with (clause 5.5.14). The recording is the one
/// every read under a map performs; a retrieval's half of it is the single
/// history it held itself, and the fact that it applied no filter and so sheds
/// nothing (clause 4.3.6.7).
///
/// # Errors
///
/// The classified failure of writing to the map store.
async fn record_retrieve_map(
    entity_maps: &EntityMapService,
    map_use: EntityMapUse,
    contributions: Vec<SourceContribution>,
    local_id: Option<Uri>,
    instance: &RequestUri,
) -> Result<Option<Uri>, ProblemResponse> {
    let local = LocalHistoryEntities::Retrieved(local_id.as_ref());
    entity_maps
        .record_read(map_use, contributions, &local, local.page_filtering())
        .await
        .map_err(|failure| match failure {
            RecordingFailure::LocalEntities(problem) => problem,
            RecordingFailure::Store(error) => error.classify_for_temporal_http().with_instance(instance.clone()), // clone: the problem response owns the request URI it names
        })
}

/// The temporal representation a retrieval answers with when a registration
/// matched but no source returned history (clause 5.7.3.4): the entity's `id`
/// and the type the registration registers it under, with no attributes.
fn registered_identity(id: &Uri, sources: &MatchedSources) -> Option<Value> {
    if sources.registrations == 0 {
        return None;
    }
    let registered_type = sources.registered_type.as_ref()?;
    Some(json!({"id": id.as_str(), "type": registered_type.as_str()}))
}
