//! Assembling the response a single-entity temporal retrieval answers with
//! (clause 5.7.3.4 and Table 6.19.3.1-1).
//!
//! What there is to answer with -- the Entity Map in use, the sources
//! contacted and the merged history -- is decided by
//! `crate::temporal::retrieve_forwarding`.

use crate::{
    error::temporal_problem,
    history_read::TemporalHistoryRead,
    join::{apply_temporal_join, single_retrieval_payload},
    temporal::{
        AcceptFormatNoGeo,
        ContextSpec,
        IntoResponse,
        NgsiLdProtocolHeaders,
        NgsiLdRequestEnvelope,
        NgsiLdResponse,
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
        retrieve_forwarding::{RetrievalSources, RetrievedHistory, distributed_retrieval, resolve_retrieve_map},
        start_rendering,
    },
};
use andromeda_entity_map::reuse::EntityMapUse;
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::context_target;
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_http_response::{collection::NgsiLdCollectionResponse, content_range::build_content_range};
use andromeda_ngsi_ld_request::join_traversal::RetrievingJoin;
use andromeda_temporal::{error::TemporalError, linked::candidates::TemporalLinkedCandidate};
use axum::http::Uri as RequestUri;

pub async fn get(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: TenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope {
        uri,
        context,
        incoming_via,
        headers,
    }: NgsiLdRequestEnvelope,
    protocol: NgsiLdProtocolHeaders,
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
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        };
        distributed_retrieval(
            RetrievalSources {
                forwarding: &state.forwarding,
                entity_maps: &state.entity_maps,
                triggering,
                map_use,
            },
            &target,
            &id,
            &params,
            &context,
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

    let content_range = range.and_then(|range| build_content_range(&range.interval, range.observed, range.last_n_origin));
    let read = TemporalHistoryRead {
        service: &state.temporal,
        target: &target,
        params: &params,
        context: &context,
    };
    let (payload, warnings) = retrieval_payload(&read, value, &linked_candidates, warnings, &uri).await?;

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
    read: &TemporalHistoryRead<'_>,
    value: Value,
    linked_candidates: &[TemporalLinkedCandidate],
    warnings: Vec<NgsiLdWarning>,
    instance: &RequestUri,
) -> Result<(ResponsePayload, Vec<NgsiLdWarning>), ProblemResponse> {
    let Some(join) = RetrievingJoin::from_mode(read.params.join) else {
        let data: Value = if read.params.is_simplified() {
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
    let warnings = apply_temporal_join(read, join, &mut roots, linked_candidates, warnings)
        .await
        .with_instance(instance)?
        .warnings;
    let payload = single_retrieval_payload(join, roots)
        .ok_or_else(|| temporal_problem(&TemporalError::internal("the join left the retrieval without its root"), instance))?;
    Ok((payload, warnings))
}
