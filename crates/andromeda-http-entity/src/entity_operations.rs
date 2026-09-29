use crate::{
    entity_error::classify_entity_error,
    entity_operations::forwarded::{
        ForwardedBatch,
        created_remotely,
        drop_absent_proxied,
        forward_entities,
        forward_identifiers,
        local_remainder,
        merge_forwarded,
    },
    error::{EntityResultExt, public_error},
    state::{EntityTenantState, EntityTenantStateOrCreate},
};
use andromeda_entity::{
    batch::UpsertPolicy,
    side_effect::{AsyncPreference, SideEffectPlacement},
};
use andromeda_federation::{provision_batch::BatchProvisionOperation, request::TriggeringRequest};
use andromeda_http_extraction::{
    accept::{AcceptFormat, AcceptFormatNoGeo},
    entity_batch::{BatchBody, parse_batch_entities},
    jsonld::JsonLd,
    prefer::PreferHeader,
    protocol::NgsiLdProtocolHeaders,
    query::ValidatedQuery,
    request_envelope::NgsiLdRequestEnvelope,
};
use andromeda_http_headers::{media_type::ResponseMediaType, preference::RespondMode, preference_applied::PreferenceApplied};
use andromeda_http_response::{
    batch::NgsiLdBatchResponse,
    batch_projection::batch_operation_result,
    problem_response::ProblemResponse,
    render::start_rendering_normalized,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{expanded::mutation::selection::OverwritePolicy, query::body::Query};
use andromeda_ngsi_ld_problem_details::{batch_operation::BatchOperationResult, error_type::NgsiLdErrorType};
use andromeda_ngsi_ld_render::response::ResponsePayload;
use andromeda_ngsi_ld_request::{
    entity_operations::{PostQueryParams, UpdateParams, UpsertParams},
    options::{OverwritePolicy as RequestOverwritePolicy, UpsertMode},
    query::entity::parameters::EntityQueryParameters,
};
use axum::{
    body::Bytes,
    http::{StatusCode, Uri as HttpUri},
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt;

fn apply_preference_applied(mut response: Response, placement: SideEffectPlacement<'_>) -> Response {
    if matches!(placement, SideEffectPlacement::DeferredOnLive) {
        response.headers_mut().typed_insert(PreferenceApplied::respond_async());
    }
    response
}

const fn async_preference(prefer: PreferHeader) -> AsyncPreference {
    match prefer.respond_mode {
        Some(RespondMode::Async) => AsyncPreference::Requested,
        Some(RespondMode::Sync) | None => AsyncPreference::NotRequested,
    }
}

/// The 207 Multi-Status answer of a batch that partly failed, carrying its
/// clause 5.2.16 result.
///
/// Clause 6.3.6: the body is always `application/json`, regardless of the
/// negotiated `Accept`, so it carries no JSON-LD context.
fn multi_status_response(
    result: &BatchOperationResult,
    context: &ContextSpec,
    uri: &HttpUri,
    placement: SideEffectPlacement<'_>,
) -> Result<Response, ProblemResponse> {
    let value = serde_json::to_value(result).with_entity_instance(uri)?;
    let rendered = start_rendering_normalized(ResponseMediaType::Json, context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Batch(value))
        .render()
        .with_entity_instance(uri)?;
    Ok(apply_preference_applied(
        NgsiLdBatchResponse::new(StatusCode::MULTI_STATUS, rendered).into_response(),
        placement,
    ))
}

/// The 201 Created answer listing the created identifiers (clauses 5.6.7.5
/// and 5.6.8.5), rendered under the negotiated media type.
fn created_response(
    created: &[Identifier],
    accept: AcceptFormatNoGeo,
    context: &ContextSpec,
    uri: &HttpUri,
    placement: SideEffectPlacement<'_>,
) -> Result<Response, ProblemResponse> {
    let value = serde_json::to_value(created).with_entity_instance(uri)?;
    let rendered = start_rendering_normalized(accept.media_type(), context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Batch(value))
        .render()
        .with_entity_instance(uri)?;
    Ok(apply_preference_applied(
        NgsiLdBatchResponse::new(StatusCode::CREATED, rendered).into_response(),
        placement,
    ))
}

/// Batch entity creation handling.
pub mod create;
/// Batch entity deletion handling.
pub mod delete;
/// The forwarded half shared by every batch operation.
mod forwarded;
/// Batch entity merge handling.
pub mod merge;
/// POST entity query handling.
pub mod query;
/// Batch entity update handling.
pub mod update;
/// Batch entity upsert handling.
pub mod upsert;
