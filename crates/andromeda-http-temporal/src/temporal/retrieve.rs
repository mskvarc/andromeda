//! HTTP handling for temporal retrieve.

use crate::{
    error::{single_source_declined, temporal_problem},
    join::{apply_temporal_join, single_retrieval_payload},
    temporal::{
        AcceptFormatNoGeo,
        ContextSpec,
        ContextUrl,
        CsrModeFilter,
        ForwardingOperation,
        IntoResponse,
        NgsiLdProtocolHeaders,
        NgsiLdResponse,
        OriginalUri,
        PreferHeader,
        ProblemResponse,
        Response,
        ResponsePayload,
        ResultInstanceExt,
        StatusCode,
        TemporalQueryParameters,
        TemporalSimplifyPayload,
        TemporalSimplifyRenderer,
        TenantState,
        TypedPath,
        Uri,
        ValidatedQuery,
        Value,
        collect_temporal_warnings,
        merge_temporal_retrieve_results,
        start_rendering,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::{
    fingerprint::{FingerprintedRequest, fingerprint_of},
    reuse::{EntityMapFlag, EntityMapUse},
    service::EntityMapService,
};
use andromeda_federation::{
    FederationService,
    entity_result::temporal_contributions_of,
    forwarded_query::ClientQuery,
    outcome::{ForwardReadOutcome, MatchedSources, SourceContribution},
    request::{EntityMapInUse, ForwardRetrieveRequest, TriggeringRequest},
    temporal_last_n::truncate_to_last_n,
};
use andromeda_http_extraction::context_target;
use andromeda_http_headers::{ngsild_entity_map::NgsiLdEntityMap, ngsild_warning::NgsiLdWarning};
use andromeda_http_response::{collection::NgsiLdCollectionResponse, content_range::build_content_range};
use andromeda_ngsi_ld_request::join_traversal::RetrievingJoin;
use andromeda_temporal::{
    error::TemporalError,
    linked::candidates::TemporalLinkedCandidate,
    service::{TemporalRange, TemporalReadResult, TemporalService},
};
use axum::http::{HeaderMap, Uri as RequestUri};
use serde_json::json;
use tracing::warn;

pub async fn get(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    protocol: NgsiLdProtocolHeaders,
    headers: HeaderMap,
    TypedPath(id): TypedPath<Uri>,
    ValidatedQuery(mut params): ValidatedQuery<TemporalQueryParameters>,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    let context: ContextSpec = context.into_inner();
    protocol.extensions().apply_to_temporal_query(&mut params).with_instance(&uri)?;
    params.validate(false).with_instance(&uri)?;

    // Clause 5.7.3.4: a local retrieval contacts no source and uses no map.
    let is_local = !target.permits_federation() || params.is_local();
    let map_use = if is_local {
        EntityMapUse::None
    } else {
        resolve_retrieve_map(&state.entity_maps, &params, protocol.entity_map.as_ref(), &uri).await?
    };
    let map_created = map_use.mints();

    let local_result = service.get(&target, &id, &params, &context).await;
    let retrieved = if is_local {
        let result = local_result.with_instance(&uri)?;
        RetrievedHistory {
            value: result.value,
            range: result.range,
            warnings: Vec::new(),
            linked_candidates: result.linked_candidates,
            entity_map_uri: None,
        }
    } else {
        let triggering = TriggeringRequest {
            via: protocol.incoming_via.as_ref(),
            headers: Some(&headers),
        };
        distributed_retrieval(
            &state.forwarding,
            &state.entity_maps,
            &target,
            &id,
            &params,
            &context,
            triggering,
            map_use,
            local_result,
            &uri,
        )
        .await?
    };
    let RetrievedHistory {
        value,
        range,
        warnings,
        linked_candidates,
        entity_map_uri,
    } = retrieved;

    let content_range = range.and_then(|range| build_content_range(&range.interval, range.observed, range.partition_total, range.last_n_origin));
    let (payload, warnings) = retrieval_payload(service, &target, &params, &context, value, &linked_candidates, warnings, &uri).await?;

    // Table 6.19.3.1-1: 201 when an EntityMap has been (re)created, 206 when
    // the history was truncated, 200 otherwise.
    let status = if map_created {
        StatusCode::CREATED
    } else if content_range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let is_collection = matches!(payload, ResponsePayload::Collection(_));
    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(payload)
        .render()
        .with_instance(&uri)?;

    // One assembly point. The two builders are distinct types, so the match is
    // on which one is being built, never on which metadata to attach.
    if is_collection {
        let mut builder = NgsiLdCollectionResponse::ok(rendered).with_status(status).with_warnings(warnings);
        if let Some(map_uri) = entity_map_uri {
            builder = builder.with_entity_map_uri(map_uri);
        }
        if let Some(range) = content_range {
            builder = builder.with_content_range(range);
        }
        return Ok(builder.into_response());
    }
    let mut builder = NgsiLdResponse::new(status, rendered).with_warnings(warnings);
    if let Some(map_uri) = entity_map_uri {
        builder = builder.with_entity_map_uri(map_uri);
    }
    if let Some(range) = content_range {
        builder = builder.with_content_range(range);
    }
    Ok(builder.into_response())
}

/// What a temporal retrieval assembled before its representation is rendered.
struct RetrievedHistory {
    /// The temporal entity the response carries.
    value: Value,
    /// Range metadata of the local read, present when it truncated the history.
    range: Option<TemporalRange>,
    /// The clause 6.3.17 warnings the forwarding produced.
    warnings: Vec<NgsiLdWarning>,
    /// The Linked Entities the local representation names, read off it before
    /// any format projection narrowed it (clause 4.5.23).
    linked_candidates: Vec<TemporalLinkedCandidate>,
    /// The Entity Map the retrieval ran under, when one applies (clause 5.5.14).
    entity_map_uri: Option<Uri>,
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
async fn resolve_retrieve_map(
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

/// Retrieve the entity's history from every matching source and this broker.
///
/// # Errors
///
/// The Table 6.4.3.2-3 `NotImplemented` answer when the one source holding all
/// the data declined the requested representation, the classified failure of
/// the local read or of the map store, and `ResourceNotFound` when the entity
/// is held neither locally nor by any matching registration (clause 5.7.3.4).
async fn distributed_retrieval(
    forwarding: &FederationService,
    entity_maps: &EntityMapService,
    target: &ContextTarget,
    id: &Uri,
    params: &TemporalQueryParameters,
    context: &ContextSpec,
    triggering: TriggeringRequest<'_>,
    map_use: EntityMapUse,
    local_result: Result<TemporalReadResult, TemporalError>,
    instance: &RequestUri,
) -> Result<RetrievedHistory, ProblemResponse> {
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
/// maps its own sources answered with (clause 5.5.14).
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
    match map_use {
        EntityMapUse::Reuse(map) => {
            let links = contributions
                .iter()
                .filter_map(|contribution| contribution.linked_map.as_ref().map(|linked| (&contribution.registration, linked)));
            entity_maps.record_linked_maps(&map.id, links).await.with_instance(instance)?;
            Ok(Some(map.id))
        }
        EntityMapUse::Mint(mut map) => {
            map.add_local_entities(local_id);
            for contribution in contributions {
                map.add_contribution(&contribution.registration, contribution.entity_ids, contribution.linked_map);
            }
            entity_maps.store(&map).await.with_instance(instance)?;
            Ok(Some(map.id))
        }
        EntityMapUse::None => Ok(None),
    }
}

/// The payload the retrieval answers with, and the warnings that accompany it.
///
/// Join mode (clause 4.5.23) is parsed at the query boundary. `@none` and an
/// absent `join` retrieve no linked entity at all, so neither reaches the
/// traversal. A flat join turns the retrieval into a collection; everything
/// else about the response — status, `Content-Range`, `NGSILD-EntityMap`,
/// warnings — is identical either way, which is why only the payload is
/// decided here and the response is assembled once by the caller.
///
/// # Errors
///
/// The classified failure of the traversal or of the `keyValues` projection,
/// and [`TemporalError::Internal`] when a join left the retrieval without its
/// root, which an inline join rewriting its root in place cannot do.
async fn retrieval_payload(
    service: &TemporalService,
    target: &ContextTarget,
    params: &TemporalQueryParameters,
    context: &ContextSpec,
    value: Value,
    linked_candidates: &[TemporalLinkedCandidate],
    warnings: Vec<NgsiLdWarning>,
    instance: &RequestUri,
) -> Result<(ResponsePayload, Vec<NgsiLdWarning>), ProblemResponse> {
    let Some(join) = RetrievingJoin::from_mode(params.join) else {
        let data: Value = if params.is_simplified() {
            TemporalSimplifyRenderer::new(TemporalSimplifyPayload::Single(value))
                .render()
                .with_instance(instance)?
                .into_single()
                .with_instance(instance)?
        } else {
            value
        };
        return Ok((ResponsePayload::Single(data), warnings));
    };
    let mut roots = vec![value];
    let warnings = apply_temporal_join(service, target, join, &mut roots, linked_candidates, params, context, warnings)
        .await
        .with_instance(instance)?
        .warnings;
    let payload = single_retrieval_payload(join, roots)
        .ok_or_else(|| temporal_problem(&TemporalError::internal("the join left the retrieval without its root"), instance))?;
    Ok((payload, warnings))
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
