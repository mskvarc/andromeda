use crate::{
    api::tenant::{TenantState, TenantStateOrCreate},
    error::{AndromedaError, ResultInstanceExt},
    http::{
        error::{batch::operation_result, problem::NgsiLdError},
        extractor::{
            accept::AcceptFormat,
            batch::{BatchBody, parse_batch_entities},
            context::ContextUrl,
            jsonld::JsonLd,
            prefer::PreferHeader,
            protocol::NgsiLdProtocolHeaders,
            query::ValidatedQuery,
            snapshot::SnapshotHeader,
            via::IncomingVia,
        },
        handler::entity_provision_forwarding::ForwardingModes,
        response::{batch::NgsiLdBatchResponse, collection::NgsiLdCollectionResponse, render::start_rendering, sonic_json::SonicJson},
    },
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
use andromeda_ngsi_ld_http_headers::{preference::RespondMode, preference_applied::PreferenceApplied};
use andromeda_ngsi_ld_model::{entity_type::EntityTypeValue, expanded::mutation::selection::OverwritePolicy, query::body::Query};
use andromeda_ngsi_ld_problem_details::batch_operation::BatchOperationResult;
use andromeda_ngsi_ld_render::{entity::EntityRenderer, response::ResponsePayload};
use andromeda_ngsi_ld_representation::join_mode::JoinMode;
use andromeda_ngsi_ld_request::{
    entity_operations::{PostQueryParams, UpdateParams, UpsertParams},
    options::{OverwritePolicy as RequestOverwritePolicy, UpsertMode},
    query::entity::parameters::EntityQueryParameters,
};
use axum::{
    body::Bytes,
    extract::OriginalUri,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt;
use serde_json::Value;
use std::{collections::HashSet, num::NonZeroU32};

fn apply_preference_applied(mut response: Response, mode: BatchSideEffectMode) -> Response {
    if matches!(mode, BatchSideEffectMode::Enqueue) {
        response.headers_mut().typed_insert(PreferenceApplied::respond_async());
    }
    response
}

fn async_preference(prefer: &PreferHeader) -> AsyncPreference {
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
