use crate::{
    error::{EntityResultExt, internal_error, public_error},
    join::{JoinRequest, ResolvedJoin},
    provision_forwarding::ForwardingModes,
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
    request::{CsrModeFilter, ForwardBatchProvisionRequest, ForwardQueryRequest},
};
use andromeda_http_extraction::{
    accept::{AcceptFormat, AcceptFormatNoGeo},
    context::ContextUrl,
    entity_batch::{BatchBody, parse_batch_entities},
    jsonld::JsonLd,
    prefer::PreferHeader,
    protocol::NgsiLdProtocolHeaders,
    query::ValidatedQuery,
    snapshot::SnapshotHeader,
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
use andromeda_ngsi_ld_model::{entity_type::EntityTypeValue, expanded::mutation::selection::OverwritePolicy, query::body::Query};
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
    Json,
    body::Bytes,
    extract::OriginalUri,
    http::StatusCode,
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

/// Handles POST query requests for batch entity retrieval (NGSI-LD 5.7.2).
/// Batch entity creation handling.
pub mod create;
/// Batch entity deletion handling.
pub mod delete;
/// Batch entity merge handling.
pub mod merge;
/// POST entity query handling.
pub mod query;
/// Batch entity update handling.
pub mod update;
/// Batch entity upsert handling.
pub mod upsert;
