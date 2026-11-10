//! The distributed half of a single-entity temporal retrieval (clause
//! 5.7.3.4): the Entity Map the retrieval runs under, the fan-out to every
//! matching registration, and the merge of what they returned onto the history
//! this broker holds itself, or, for an aggregated representation of an entity
//! held in more than one part, its aggregation here over every part.
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
        Identifier,
        ProblemResponse,
        ResultInstanceExt,
        TemporalQueryParameters,
        collect_temporal_warnings,
        complete_history::HistoryCompleteness,
        local_map_entities::LocalHistoryEntities,
        retrieve_aggregation::{RetrievalAggregation, RetrievalRound},
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::{
    contribution::SourceContribution,
    fingerprint::{FingerprintedRequest, fingerprint_of},
    recording::{PageMatches, RecordingFailure},
    reuse::{EntityMapFlag, EntityMapUse},
    service::EntityMapService,
};
use andromeda_federation::{
    FederationService,
    entity_result::temporal_contributions_of,
    forwarded_query::client_query::{ClientQuery, TemporalTraversal},
    request::{EntityMapInUse, ForwardRetrieveRequest, TriggeringRequest},
    source_reach::SourceReach,
};
use andromeda_http_headers::{ngsild_entity_map::NgsiLdEntityMap, ngsild_warning::NgsiLdWarning};
use andromeda_http_response::declined_representation::single_source_declined;
use andromeda_ngsi_ld_model::temporal::temporal_history::TemporalHistory;
use andromeda_ngsi_ld_query::projection::compiled::Projection;
use andromeda_temporal::{
    error::TemporalError,
    service::{TemporalRange, TemporalReadResult, TemporalService},
    temporal_not_found_reason::TemporalNotFoundReason,
};
use axum::http::Uri as RequestUri;

/// What a temporal retrieval assembled before its representation is rendered.
pub(crate) struct RetrievedHistory {
    /// The history of the entity the response carries, merged over every
    /// holder, before it is compacted under the request context.
    pub(crate) history: TemporalHistory,
    /// Range metadata of the merged history, present when the local read or
    /// a source cut it short.
    pub(crate) range: Option<TemporalRange>,
    /// The clause 6.3.17 warnings the forwarding produced.
    pub(crate) warnings: Vec<NgsiLdWarning>,
    /// The request's clause 4.21 projection, which narrows the members the
    /// response writes of the entity.
    pub(crate) projection: Projection,
    /// The Entity Map the retrieval ran under, when one applies (clause 5.5.14).
    pub(crate) entity_map_uri: Option<Identifier>,
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
    /// The temporal service, which reads the local history and the request's
    /// projection.
    pub(crate) service: &'a TemporalService,
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
/// the data declined the requested representation, the clause 4.12
/// `TooManyResults` refusal when the sources answer with more instances than
/// one request gathers, the classified failure of the local read, of an
/// aggregation here or of the map store, the 503 of a registration store that
/// is saturated or unreachable, and `ResourceNotFound` when the entity
/// is held neither locally nor by any matching registration (clause 5.7.3.4).
pub(crate) async fn distributed_retrieval(
    sources: RetrievalSources<'_>,
    target: &ContextTarget,
    id: &Identifier,
    params: &TemporalQueryParameters,
    context: &ContextSpec,
    local_result: Result<TemporalReadResult, TemporalError>,
    instance: &RequestUri,
) -> Result<RetrievedHistory, ProblemResponse> {
    let RetrievalSources {
        service,
        forwarding,
        entity_maps,
        triggering,
        map_use,
    } = sources;
    let aggregation = RetrievalAggregation::of(params, forwarding.aggregation_gathering());
    let round = RetrievalRound {
        service,
        forwarding,
        target,
        request: ForwardRetrieveRequest {
            entity_id: id,
            reach: SourceReach::Every,
            client_query: ClientQuery::TemporalRetrieve {
                id,
                params,
                traversal: TemporalTraversal::WalkedHere,
            },
            context,
            triggering,
            mode_filter: CsrModeFilter::All,
            entity_map: EntityMapInUse::from(&map_use),
        },
        params,
        instance,
    };
    let forwarded = round.first(aggregation).await?;
    if let Some(status) = forwarded.sources.single_source_status {
        return Err(single_source_declined(status, instance));
    }
    let mut warnings = collect_temporal_warnings(&forwarded.results);
    // Capture which registration returned which entity, and with which map,
    // before the merge (clauses 5.2.39 and 5.5.14).
    let contributions: Vec<SourceContribution> = temporal_contributions_of(&forwarded.results).collect();

    // Clauses 5.6.12.4 and 5.7.3.4: a local not-found beside a matching
    // registration is nothing held here, not an error.
    let (local_entity, range, local_projection) = match local_result {
        Ok(result) => (Some(result.history), result.range, Some(result.projection)),
        Err(ref error) if error.is_not_found() => (None, None, None),
        Err(error) => return Err(temporal_problem(&error, instance)),
    };

    let had_remote_history = forwarded.results.iter().any(|result| !result.entities.is_empty());
    let local_id = local_entity.is_some().then(|| id.clone()); // clone: `id` is used again afterwards, so this Option takes its own copy
    let window = params.window().map_err(|error| temporal_problem(&TemporalError::from(error), instance))?;
    // Clause 6.3.10: a source that cut its history short bounds the merged
    // page as the local cut does, so the response's range is the merge's.
    let mut completeness = HistoryCompleteness::of(params, range, &forwarded.results).map_err(|error| temporal_problem(&error, instance))?;
    let (merged, assembly_warnings) = round.put_together(aggregation, local_entity, forwarded.results, window).await?;
    warnings.extend(assembly_warnings);
    let entity_map_uri = record_retrieve_map(entity_maps, map_use, contributions, local_id, instance).await?;

    let (history, range) = match merged {
        Some(mut history) => {
            // Clause 5.7.3.4: `lastN` bounds the instances of the merged
            // history, so it is applied once more over the aggregation.
            if let (true, Some(last_n)) = (had_remote_history, params.last_n) {
                history.keep_last(window.timeproperty, last_n);
            }
            let mut page = [history];
            warnings.extend(completeness.trim(&mut page, window.timeproperty));
            let range = completeness.range(&page, window.timeproperty);
            let [history] = page;
            (history, range)
        }
        // Clause 5.7.3.4: ResourceNotFound when the target Entity's history is
        // held neither locally nor by any source a matching registration
        // forwarded to. A registration that matched but whose sources returned
        // no history leaves nothing to answer with. The broker does not
        // invent a temporal representation from the registration's id and type,
        // and clause 6.3.17 does not treat a source's empty answer as abnormal.
        None => {
            return Err(temporal_problem(&TemporalError::NotFound(TemporalNotFoundReason::Entity { id: id.clone() }), instance).with_warnings(warnings));
        }
    };
    // The local read compiled the request's projection already; a history only
    // a source holds still answers under it.
    let projection = match local_projection {
        Some(projection) => projection,
        None => service
            .request_projection(params, context)
            .await
            .map_err(|error| temporal_problem(&error, instance))?,
    };
    Ok(RetrievedHistory {
        history,
        range,
        warnings,
        projection,
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
    local_id: Option<Identifier>,
    instance: &RequestUri,
) -> Result<Option<Identifier>, ProblemResponse> {
    let local = LocalHistoryEntities::Retrieved(local_id.as_ref());
    entity_maps
        // A retrieval applies no filters, so it prunes nothing and the match set
        // is never consulted; the per-source reading is all it needs.
        .record_read(map_use, contributions, &local, local.page_filtering(), PageMatches::PerSource)
        .await
        .map_err(|failure| match failure {
            RecordingFailure::LocalEntities(problem) => problem,
            RecordingFailure::Store(error) => ProblemResponse::classified(error.classify_for_temporal_http(), instance.clone()), // clone: the problem response owns the request URI it names
        })
}
