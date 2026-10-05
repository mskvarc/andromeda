//! Assembling the response a single-entity temporal retrieval answers with
//! (clause 5.7.3.4 and Table 6.19.3.1-1).
//!
//! What there is to answer with -- the Entity Map in use, the sources
//! contacted and the merged history -- is decided by
//! `crate::temporal::retrieve_forwarding`.

use crate::{
    error::temporal_problem,
    extension_gate::TemporalExtensionGate,
    history_read::TemporalHistoryRead,
    join::{apply_temporal_join, single_retrieval_payload},
    request_context_admission::admit_request_context,
    temporal::{
        AcceptFormatNoGeo,
        Identifier,
        IntoResponse,
        NgsiLdProtocolHeaders,
        NgsiLdRequestEnvelope,
        NgsiLdResponse,
        ProblemResponse,
        Response,
        ResponseContextSource,
        ResponsePayload,
        ResultInstanceExt,
        StatusCode,
        TemporalSimplifyPayload,
        TemporalSimplifyRenderer,
        TenantState,
        TypedPath,
        Value,
        retrieve_forwarding::{RetrievalSources, RetrievedHistory, distributed_retrieval, resolve_retrieve_map},
    },
};
use andromeda_entity_map::reuse::EntityMapUse;
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::{admitted_temporal_query::AdmittedTemporalQuery, context_target, response_context_request::ResponseContextRequest};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_http_response::{collection::NgsiLdCollectionResponse, content_range::build_content_range};
use andromeda_linked_retrieval::edge::LinkedEdge;
use andromeda_ngsi_ld_request::query::temporal::operation::TemporalQueryOperation;
use andromeda_temporal::{error::TemporalError, temporal_invariant::TemporalInvariant};
use axum::http::Uri as RequestUri;

pub async fn get(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: TenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    ResponseContextRequest { context, api_root }: ResponseContextRequest,
    protocol: NgsiLdProtocolHeaders,
    TypedPath(id): TypedPath<Identifier>,
    AdmittedTemporalQuery { mut params, .. }: AdmittedTemporalQuery<TemporalExtensionGate>,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    // Clause 5.5.7: a request `@context` declaring a JSON-LD Scoped Context is
    // refused on the temporal history read path before anything is forwarded or
    // expanded against it.
    admit_request_context(service.engine(), &context, &uri).await?;
    protocol.extensions().apply_to_temporal_query(&mut params).with_instance(&uri)?;
    params
        .validate(TemporalQueryOperation::Retrieve, service.linked_retrieval())
        .with_instance(&uri)?;

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
            linked_edges: result.linked_edges,
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
        linked_edges,
        entity_map_uri,
    } = retrieved;

    let content_range = range.and_then(|range| build_content_range(&range.interval, range.observed, range.direction));
    let read = TemporalHistoryRead {
        service: &state.temporal,
        target: &target,
        params: &params,
        context: &context,
    };
    let (payload, warnings) = retrieval_payload(&read, value, &linked_edges, warnings, &uri).await?;

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
    // Table 6.19.3.1-2 carries the 203 row and clause 6.3.6 the
    // `Preference-Applied` echo, so the preference the request expressed is the
    // one that renders; `NgsiLdResponse` turns the outcome into both.
    let rendered = ResponseContextSource::new(state.temporal.engine(), &context, &state.context_hosting, &api_root)
        .start_rendering_temporal(accept.media_type(), protocol.prefer.rendering_preferences(), params.selected_format())
        .await
        .with_instance(&uri)?
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
/// `@none` and an absent `join` retrieve no linked entity at all (clause
/// 4.5.23), and the retrieval answers with its one entity, simplified when
/// the request asked for it. A flat join turns the retrieval into a
/// collection; everything else about the response -- status, `Content-Range`,
/// `NGSILD-EntityMap`, warnings -- is identical either way, which is why only
/// the payload is decided here and the response is assembled once by the
/// caller.
///
/// # Errors
///
/// The classified failure of the traversal or of the `keyValues` projection,
/// and [`TemporalError::Internal`] when a join left the retrieval without its
/// root, which an inline join rewriting its root in place cannot do.
async fn retrieval_payload(
    read: &TemporalHistoryRead<'_>,
    value: Value,
    linked_edges: &[LinkedEdge],
    warnings: Vec<NgsiLdWarning>,
    instance: &RequestUri,
) -> Result<(ResponsePayload, Vec<NgsiLdWarning>), ProblemResponse> {
    let mut roots = vec![value];
    let applied = apply_temporal_join(read, &mut roots, linked_edges, warnings).await.with_instance(instance)?;
    let without_root = || temporal_problem(&TemporalError::from(TemporalInvariant::RetrievalWithoutRoot), instance);
    if let Some(shape) = applied.shape {
        let payload = single_retrieval_payload(shape, roots).ok_or_else(without_root)?;
        return Ok((payload, applied.warnings));
    }
    let value = roots.into_iter().next().ok_or_else(without_root)?;
    let data: Value = if read.params.is_simplified() {
        TemporalSimplifyRenderer::new(TemporalSimplifyPayload::Single(value))
            .render()
            .with_instance(instance)?
            .into_single()
            .with_instance(instance)?
    } else {
        value
    };
    let payload = ResponsePayload::Single(data);
    Ok((payload, applied.warnings))
}
