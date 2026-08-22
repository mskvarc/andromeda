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
use andromeda_entity_map::{
    fingerprint::{FingerprintedRequest, fingerprint_of},
    reuse::{EntityMapFlag, EntityMapUse},
};
use andromeda_federation::{
    entity_result::temporal_contributions_of,
    forwarded_query::ClientQuery,
    outcome::{ForwardReadOutcome, MatchedSources, SourceContribution},
    request::{EntityMapInUse, ForwardRetrieveRequest, TriggeringRequest},
    temporal_last_n::truncate_to_last_n,
};
use andromeda_http_extraction::tenant_state;
use andromeda_http_headers::ngsild_entity_map::NgsiLdEntityMap;
use andromeda_http_response::{collection::NgsiLdCollectionResponse, content_range::build_content_range};
use andromeda_ngsi_ld_request::join_traversal::RetrievingJoin;
use andromeda_temporal::error::TemporalError;
use axum::http::HeaderMap;
use serde_json::json;
use tracing::warn;

pub async fn get(
    tenant_state::ExistingTenantState(state): TenantState,
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
    let snapshot = protocol.snapshot;
    let incoming_via = protocol.incoming_via;
    let entity_map_header = protocol.entity_map;

    // Snapshot scoping (ETSI GS CIM 009 clause 5.5.15, 6.3.22): if NGSILD-Snapshot header
    // is present, retrieve temporal entity from the snapshot's frozen data and return early.
    if let Some(snapshot_id) = snapshot.into_inner() {
        state.snapshots.validate(&snapshot_id).await.with_instance(&uri)?;

        let entity_data = state
            .snapshots
            .entity(&snapshot_id, &id)
            .await
            .with_instance(&uri)?
            .ok_or_else(|| temporal_problem(&TemporalError::not_found(id.to_string()), &uri))?;

        let key_values = params.is_simplified();

        let data = if key_values {
            TemporalSimplifyRenderer::new(TemporalSimplifyPayload::Single(entity_data))
                .render()
                .with_instance(&uri)?
                .into_single()
                .with_instance(&uri)?
        } else {
            entity_data
        };
        let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
            .with_payload(ResponsePayload::Single(data))
            .render()
            .with_instance(&uri)?;
        return Ok(NgsiLdResponse::ok(rendered).with_snapshot_uri(snapshot_id).into_response());
    }

    let is_local = params.is_local();

    // Clause 5.7.3.4: a referenced live EntityMap fixes the registrations the
    // retrieval contacts; an expired or unknown one yields no inference, so a
    // fresh map is created instead. A local retrieval contacts no source and
    // uses no map.
    let map_use = if is_local {
        EntityMapUse::None
    } else {
        let fingerprint = fingerprint_of(FingerprintedRequest::RetrieveTemporal(&params)).with_instance(&uri)?;
        state
            .entity_maps
            .resolve_use(
                entity_map_header.as_ref().map(NgsiLdEntityMap::as_inner),
                EntityMapFlag::from_parameter(params.entity_map),
                &fingerprint,
                params.entity_map_lifetime.as_ref(),
            )
            .await
            .with_instance(&uri)?
    };
    let map_created = map_use.mints();
    let mut entity_map_uri = None;

    let local_result = service.get(&id, &params, &context).await;

    let (entity_value, range, warnings, linked_candidates) = if is_local {
        let result = local_result.with_instance(&uri)?;
        (result.value, result.range, Vec::new(), result.linked_candidates)
    } else {
        let forwarded = state
            .forwarding
            .forward_temporal_retrieve(ForwardRetrieveRequest {
                entity_id: &id,
                exclude_registration: None,
                client_query: ClientQuery::Temporal(&params),
                context: &context,
                operation: ForwardingOperation::RetrieveTemporalEntity,
                triggering: TriggeringRequest {
                    via: incoming_via.as_ref(),
                    headers: Some(&headers),
                },
                mode_filter: CsrModeFilter::All,
                entity_map: EntityMapInUse::from(&map_use),
            })
            .await
            .unwrap_or_else(|error| {
                warn!(%error, "context source forwarding failed during the temporal retrieval");
                ForwardReadOutcome::empty()
            });
        if let Some(status) = forwarded.sources.single_source_status {
            return Err(single_source_declined(status, &uri));
        }
        let warnings = collect_temporal_warnings(&forwarded.results);
        let contributions: Vec<SourceContribution> = temporal_contributions_of(&forwarded.results).collect();

        let local_value = match local_result {
            Ok(result) => Some((result.value, result.range, result.linked_candidates)),
            Err(ref e) if e.is_not_found() => None,
            Err(e) => return Err(temporal_problem(&e, &uri)),
        };

        let (local_entity, range, linked_candidates) = match local_value {
            Some((value, cr, candidates)) => (Some(value), cr, candidates),
            None => (None, None, Vec::new()),
        };

        let had_remote_history = forwarded.results.iter().any(|result| !result.entities.is_empty());
        let held_locally = local_entity.is_some();
        let merged = merge_temporal_retrieve_results(local_entity, forwarded.results);

        // Table 6.19.3.1-1 `entityMap`: the map the retrieval used is
        // returned, created now when none was in use (clause 5.7.3.4), naming
        // this broker and every registration that returned history; a reused
        // map records the maps its sources answered with (clause 5.5.14).
        entity_map_uri = match map_use {
            EntityMapUse::Reuse(map) => {
                let links = contributions
                    .iter()
                    .filter_map(|contribution| contribution.linked_map.as_ref().map(|linked| (&contribution.registration, linked)));
                state.entity_maps.record_linked_maps(&map.id, links).await.with_instance(&uri)?;
                Some(map.id)
            }
            EntityMapUse::Mint(mut map) => {
                map.add_local_entities(held_locally.then(|| id.clone())); // clone: the map owns its entity identifiers
                for contribution in contributions {
                    map.add_contribution(&contribution.registration, contribution.entity_ids, contribution.linked_map);
                }
                state.entity_maps.store(&map).await.with_instance(&uri)?;
                Some(map.id)
            }
            EntityMapUse::None => None,
        };
        match merged {
            Some(mut value) => {
                // Clause 5.7.3.4: `lastN` bounds the instances of the merged
                // history, so it is applied once more over the aggregation.
                if let (true, Some(last_n)) = (had_remote_history, params.last_n) {
                    truncate_to_last_n(&mut value, last_n, params.timeproperty_or_default());
                }
                (value, range, warnings, linked_candidates)
            }
            // Clause 5.7.3.4: ResourceNotFound only when nothing is held
            // locally and no registration matches; a matching registration
            // that returned nothing leaves the entity known by its registered
            // identity alone.
            None => match registered_identity(&id, &forwarded.sources) {
                Some(value) => (value, range, warnings, linked_candidates),
                None => {
                    return Err(temporal_problem(&TemporalError::not_found(id.to_string()), &uri).with_warnings(warnings));
                }
            },
        }
    };

    let content_range = range.and_then(|range| build_content_range(&range.interval, range.observed, range.partition_total, range.last_n_origin));

    // Join mode (clause 4.5.23) is parsed at the query boundary. `@none` and an
    // absent `join` retrieve no linked entity at all, so neither reaches the
    // traversal. A flat join turns the retrieval into a collection; everything
    // else about the response — status, `Content-Range`, `NGSILD-EntityMap`,
    // warnings — is identical either way, which is why only the payload is
    // decided here and the response is assembled once below.
    let (payload, warnings) = match RetrievingJoin::from_mode(params.join) {
        Some(join) => {
            let mut roots = vec![entity_value];
            let warnings = apply_temporal_join(service, join, &mut roots, &linked_candidates, &params, &context, warnings)
                .await
                .with_instance(&uri)?
                .warnings;
            let payload = single_retrieval_payload(join, roots)
                .ok_or_else(|| temporal_problem(&TemporalError::internal("the join left the retrieval without its root"), &uri))?;
            (payload, warnings)
        }
        None => {
            let data: Value = if params.is_simplified() {
                TemporalSimplifyRenderer::new(TemporalSimplifyPayload::Single(entity_value))
                    .render()
                    .with_instance(&uri)?
                    .into_single()
                    .with_instance(&uri)?
            } else {
                entity_value
            };
            (ResponsePayload::Single(data), warnings)
        }
    };

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
