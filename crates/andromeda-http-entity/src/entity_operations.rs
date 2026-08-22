use crate::{
    entity_error::classify_entity_error,
    entity_operations::forwarded::{created_remotely, drop_absent_proxied, forward_entities, forward_identifiers, local_remainder, merge_forwarded},
    error::{EntityResultExt, public_error},
    join::{JoinRequest, ResolvedJoin},
    request_projection::RequestProjection,
    state::{EntityTenantState, EntityTenantStateOrCreate},
};
use andromeda_entity::{
    batch::UpsertPolicy,
    query::EntityQueryService,
    side_effect::{AsyncPreference, BatchSideEffectMode},
};
use andromeda_federation::{
    entity_result::collect_warnings,
    operation::ForwardingOperation,
    provision_batch::BatchProvisionOperation,
    request::{CsrModeFilter, ForwardQueryRequest, TriggeringRequest},
};
use andromeda_http_extraction::{
    accept::{AcceptFormat, AcceptFormatNoGeo},
    context::ContextUrl,
    entity_batch::{BatchBody, parse_batch_entities},
    jsonld::JsonLd,
    prefer::PreferHeader,
    protocol::NgsiLdProtocolHeaders,
    query::ValidatedQuery,
    via::IncomingVia,
};
use andromeda_http_headers::{media_type::ResponseMediaType, preference::RespondMode, preference_applied::PreferenceApplied};
use andromeda_http_response::{
    batch::NgsiLdBatchResponse,
    batch_projection::batch_operation_result,
    collection::NgsiLdCollectionResponse,
    linked_resolution_warning::with_linked_resolution_warnings,
    problem_response::ProblemResponse,
    render::start_rendering,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{expanded::mutation::selection::OverwritePolicy, query::body::Query};
use andromeda_ngsi_ld_problem_details::{batch_operation::BatchOperationResult, error_type::NgsiLdErrorType};
use andromeda_ngsi_ld_render::{
    entity::{EntityRenderer, render_flat_join},
    response::ResponsePayload,
};
use andromeda_ngsi_ld_request::{
    entity_operations::{PostQueryParams, UpdateParams, UpsertParams},
    join_traversal::RetrievingJoin,
    options::{OverwritePolicy as RequestOverwritePolicy, UpsertMode},
    query::entity::parameters::EntityQueryParameters,
};
use axum::{
    body::Bytes,
    extract::OriginalUri,
    http::{StatusCode, Uri as HttpUri},
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt;
use serde_json::Value;

fn apply_preference_applied(mut response: Response, mode: BatchSideEffectMode) -> Response {
    if matches!(mode, BatchSideEffectMode::Enqueue) {
        response.headers_mut().typed_insert(PreferenceApplied::respond_async());
    }
    response
}

const fn async_preference(prefer: &PreferHeader) -> AsyncPreference {
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
fn multi_status_response(result: &BatchOperationResult, context: &ContextSpec, uri: &HttpUri, mode: BatchSideEffectMode) -> Result<Response, ProblemResponse> {
    let value = serde_json::to_value(result).with_entity_instance(uri)?;
    let rendered = start_rendering(ResponseMediaType::Json, context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Batch(value))
        .render()
        .with_entity_instance(uri)?;
    Ok(apply_preference_applied(
        NgsiLdBatchResponse::new(StatusCode::MULTI_STATUS, rendered).into_response(),
        mode,
    ))
}

/// The 201 Created answer listing the created identifiers (clauses 5.6.7.5
/// and 5.6.8.5), rendered under the negotiated media type.
fn created_response(
    created: &[Uri],
    accept: &AcceptFormatNoGeo,
    context: &ContextSpec,
    uri: &HttpUri,
    mode: BatchSideEffectMode,
) -> Result<Response, ProblemResponse> {
    let value = serde_json::to_value(created).with_entity_instance(uri)?;
    let rendered = start_rendering(accept.media_type(), context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Batch(value))
        .render()
        .with_entity_instance(uri)?;
    Ok(apply_preference_applied(
        NgsiLdBatchResponse::new(StatusCode::CREATED, rendered).into_response(),
        mode,
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
