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
    join::apply_temporal_join,
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
        ResultInstanceExt,
        StatusCode,
        TenantState,
        TypedPath,
        retrieve_forwarding::{RetrievalSources, RetrievedHistory, distributed_retrieval, resolve_retrieve_map},
    },
    temporal_answer::{RetrievedAnswer, retrieved_history},
};
use andromeda_entity_map::reuse::EntityMapUse;
use andromeda_federation::request::TriggeringRequest;
use andromeda_http_extraction::{
    admitted_temporal_query::AdmittedTemporalQuery,
    context_target,
    path_segment::EntityId,
    response_context_request::ResponseContextRequest,
};
use andromeda_http_headers::{ngsild_content_range::NgsiLdContentRange, ngsild_warning::NgsiLdWarning};
use andromeda_http_response::{
    collection::NgsiLdCollectionResponse,
    content_range::build_content_range,
    rendered_response::RenderedResponse,
    temporal_continuation::append_continuation_link,
};
use andromeda_jsonld::user_context_admission::SubmittedContext;
use andromeda_ngsi_ld_query::temporal::continuation::TemporalContinuation;
use andromeda_ngsi_ld_render::structural_projection::StructuralProjection;
use andromeda_ngsi_ld_request::query::temporal::{operation::TemporalQueryOperation, retrieve_parameters::TemporalRetrieveParameters};
use andromeda_temporal::{error::TemporalError, service::TemporalRange, temporal_invariant::TemporalInvariant};

/// Retrieve Temporal Evolution of an Entity (clause 5.7.3): 201 when an
/// `EntityMap` was created, 206 when the history was truncated, 200 otherwise.
pub(crate) async fn get(
    context_target::ExistingTargetedTenantState {
        services: state,
        target,
        snapshot: _,
    }: TenantState,
    accept: AcceptFormatNoGeo,
    NgsiLdRequestEnvelope { uri, incoming_via, headers }: NgsiLdRequestEnvelope,
    ResponseContextRequest { context, api_root }: ResponseContextRequest,
    protocol: NgsiLdProtocolHeaders,
    TypedPath(EntityId(id)): TypedPath<EntityId>,
    AdmittedTemporalQuery { mut params, .. }: AdmittedTemporalQuery<TemporalExtensionGate, TemporalRetrieveParameters>,
) -> Result<Response, ProblemResponse> {
    let service = &state.temporal;
    // Clause 5.5.7: the request `@context` is admitted on the temporal history
    // read path before anything is forwarded or expanded against it.
    let context = admit_request_context(service.engine(), SubmittedContext::from(context), &uri).await?;
    params.lang = params.lang.take().or(protocol.language_filter);
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
            history: result.history,
            range: result.range,
            warnings: Vec::new(),
            projection: result.projection,
            entity_map_uri: None,
        }
    } else {
        let triggering = TriggeringRequest {
            via: incoming_via.as_ref(),
            headers: Some(&headers),
        };
        distributed_retrieval(
            RetrievalSources {
                service,
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
        history,
        range,
        warnings,
        projection,
        entity_map_uri,
    } = retrieved;

    let (content_range, next) = page_position(range);
    let read = TemporalHistoryRead {
        service: &state.temporal,
        target: &target,
        params: &params,
        context: &context,
        // A retrieval carries no `q` (Table 6.19.3.1-1).
        language_targeting: state.temporal.language_targeting(),
    };
    // The history is compacted under the request context once every holder's
    // part has been merged into it (clause 5.7.3); the join then reads the
    // compact roots.
    let bodies = service.compact_histories(vec![history], &context).await.with_instance(&uri)?;
    let (resolved, warnings) = apply_temporal_join(&read, &bodies, warnings).await.with_instance(&uri)?;
    let plan = resolved.plan();
    let Some(body) = bodies.first() else {
        return Err(temporal_problem(&TemporalError::from(TemporalInvariant::RetrievalWithoutRoot), &uri));
    };
    let answer = retrieved_history(body, plan.as_ref(), Some(StructuralProjection::from(&projection)));
    let (answer_shape, answer) = match answer {
        RetrievedAnswer::Single(answer) => (AnswerShape::Single, answer),
        RetrievedAnswer::Collection(answer) => (AnswerShape::Collection, answer),
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
    // Table 6.19.3.1-2 carries the 203 row and clause 6.3.6 the
    // `Preference-Applied` echo, so the preference the request expressed is the
    // one that renders; `NgsiLdResponse` turns the outcome into both.
    let rendering = ResponseContextSource::new(state.temporal.engine(), &context, &state.context_hosting, &api_root)
        .start_rendering_normalized(accept.media_type(), protocol.prefer.rendering_preferences())
        .await
        .with_instance(&uri)?;
    let rendered = answer.render(rendering, &params, &uri)?;

    let mut response = retrieval_response(
        answer_shape,
        RetrievalHead {
            status,
            warnings,
            entity_map_uri,
            content_range,
        },
        rendered,
    );
    if let Some(next) = next {
        append_continuation_link(&mut response, &uri, &next, accept.media_type());
    }
    Ok(response)
}

/// The `Content-Range` of a history `range` says was cut short, and the
/// request that continues it (clause 6.3.10).
///
/// Clause 6.3.10 defines no request resuming a partial response, so a page
/// that left instances out points at one that serves none of its own again;
/// a page with no range to report points nowhere.
fn page_position(range: Option<TemporalRange>) -> (Option<NgsiLdContentRange>, Option<TemporalContinuation>) {
    let Some(range) = range else {
        return (None, None);
    };
    let content_range = build_content_range(&range.interval, range.observed, range.direction);
    let next = range.next.filter(|_| content_range.is_some());
    (content_range, next)
}

/// What a retrieval says about its body beside the body.
struct RetrievalHead {
    /// Table 6.19.3.1-1: 201 when an Entity Map has been (re)created, 206 when
    /// the history was truncated, 200 otherwise.
    status: StatusCode,
    /// The clause 6.3.17 warnings of the read.
    warnings: Vec<NgsiLdWarning>,
    /// The Entity Map the retrieval ran under, when one applies (clause 5.5.14).
    entity_map_uri: Option<Identifier>,
    /// The `Content-Range` of a history that was cut short.
    content_range: Option<NgsiLdContentRange>,
}

/// The response a retrieval answers with.
///
/// One assembly point: the two builders are distinct types, so the match is on
/// which one is being built, never on which metadata to attach.
fn retrieval_response(shape: AnswerShape, head: RetrievalHead, rendered: RenderedResponse) -> Response {
    let RetrievalHead {
        status,
        warnings,
        entity_map_uri,
        content_range,
    } = head;
    match shape {
        AnswerShape::Collection => {
            let mut builder = NgsiLdCollectionResponse::ok(rendered).with_status(status).with_warnings(warnings);
            if let Some(map_uri) = entity_map_uri {
                builder = builder.with_entity_map_uri(map_uri);
            }
            if let Some(range) = content_range {
                builder = builder.with_content_range(range);
            }
            builder.into_response()
        }
        AnswerShape::Single => {
            let mut builder = NgsiLdResponse::new(status, rendered).with_warnings(warnings);
            if let Some(map_uri) = entity_map_uri {
                builder = builder.with_entity_map_uri(map_uri);
            }
            if let Some(range) = content_range {
                builder = builder.with_content_range(range);
            }
            builder.into_response()
        }
    }
}

/// Whether a retrieval answers one entity or, joined flat, a collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnswerShape {
    /// The entity itself.
    Single,
    /// The entity beside the Linked Entities a flat join reached.
    Collection,
}
