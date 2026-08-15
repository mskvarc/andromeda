//! HTTP handling for temporal retrieve.

use crate::{
    error::temporal_problem,
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
use andromeda_http_extraction::tenant_state;
use andromeda_http_headers::ngsild_entity_map::NgsiLdEntityMap;
use andromeda_http_response::{collection::NgsiLdCollectionResponse, content_range::build_content_range};
use andromeda_ngsi_ld_request::join_traversal::RetrievingJoin;
use andromeda_temporal::error::TemporalError;

pub async fn get(
    tenant_state::ExistingTenantState(state): TenantState,
    OriginalUri(uri): OriginalUri,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
    protocol: NgsiLdProtocolHeaders,
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

    // EntityMap scoping (spec 5.7.3.4): if NGSILD-EntityMap header is present,
    // only entities in the map are accessible. Return 404 if not in the map.
    if let Some(ref map_id) = entity_map_header {
        state.entity_maps.validate_entity_in_map(map_id.as_inner(), &id).await.with_instance(&uri)?;
    }

    let is_local = params.is_local();

    let local_result = service.get(&id, &params, &context).await;

    let (entity_value, range, warnings, linked_candidates) = if is_local {
        let result = local_result.with_instance(&uri)?;
        (result.value, result.range, Vec::new(), result.linked_candidates)
    } else {
        let context_url = context.link_contexts().first();
        let query_string = uri.query().unwrap_or("");
        let forwarded = state
            .forwarding
            .forward_temporal_retrieve(
                &id,
                query_string,
                context_url,
                ForwardingOperation::RetrieveTemporalEntity,
                incoming_via.as_ref(),
                CsrModeFilter::All,
            )
            .await
            .unwrap_or_default();
        let warnings = collect_temporal_warnings(&forwarded);

        let local_value = match local_result {
            Ok(result) => Some((result.value, result.range, result.linked_candidates)),
            Err(ref e) if e.is_not_found() => None,
            Err(e) => return Err(temporal_problem(&e, &uri)),
        };

        let (local_entity, range, linked_candidates) = match local_value {
            Some((value, cr, candidates)) => (Some(value), cr, candidates),
            None => (None, None, Vec::new()),
        };

        let merged = merge_temporal_retrieve_results(local_entity, forwarded);
        match merged {
            Some(value) => (value, range, warnings, linked_candidates),
            None => {
                return Err(temporal_problem(&TemporalError::not_found(id.to_string()), &uri));
            }
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

    let status = if content_range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let is_collection = matches!(payload, ResponsePayload::Collection(_));
    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(payload)
        .render()
        .with_instance(&uri)?;
    let entity_map_uri = entity_map_header.map(NgsiLdEntityMap::into_inner);

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
