use crate::{
    error::{EntityResultExt, internal_error, public_error},
    provision_forwarding::{ForwardingModes, build_multi_status_body, first_forwarded_status, has_remote_errors},
    state::{EntityTenantState, EntityTenantStateOrCreate},
};
use andromeda_entity::query::EntityQueryService;
use andromeda_federation::{
    entity_result::{collect_forwarded_exclusive_attrs, collect_warnings, merge_retrieve_results, strip_exclusive_attrs_from_value},
    operation::ForwardingOperation,
    request::{CsrModeFilter, ForwardBatchProvisionRequest, ForwardProvisionRequest, ForwardQueryRequest},
};
use andromeda_http_extraction::{
    accept::AcceptFormat,
    context::ContextUrl,
    entity_query::EntityQuery,
    jsonld::JsonLd,
    protocol::NgsiLdProtocolHeaders,
    query::ValidatedQuery,
    typed_path::TypedPath,
    via::IncomingVia,
};
use andromeda_http_response::{
    batch_projection::{batch_entity_error, batch_operation_result},
    collection::NgsiLdCollectionResponse,
    problem_response::ProblemResponse,
    render::start_rendering,
    single::NgsiLdResponse,
};
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    entity::{Entity, EntityPatch},
    entity_type::EntityTypeValue,
    expanded::{
        mutation::selection::{DatasetSelection, OverwritePolicy},
        query_match::{matches_geo_query, matches_scope_query},
    },
};
use andromeda_ngsi_ld_problem_details::{batch_operation::BatchOperationResult, error_type::NgsiLdErrorType};
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    filter::{eval::evaluate, expansion::expand_q_expr},
    geo::compile::compile_geo_query,
    projection::purge::PurgeMode,
};
use andromeda_ngsi_ld_render::{
    entity::{EntityRenderer, render_flat_join},
    response::ResponsePayload,
    snapshot::SnapshotRenderer,
};
use andromeda_ngsi_ld_representation::join_mode::JoinMode;
use andromeda_ngsi_ld_request::{
    attribute_deletion::DeleteAttrParams,
    deferred_split_filters::DeferredSplitFilters,
    entity::{AppendParams, GetEntityParams, PartialUpdateAttrParams, ReplaceAttrParams},
    local::LocalOnlyParams,
    options::OverwritePolicy as RequestOverwritePolicy,
};
use axum::{
    Json,
    extract::OriginalUri,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::Value;
use std::{collections::HashSet, num::NonZeroU32};

const fn selected_dataset_uri(selector: Option<&DatasetSelector>) -> Option<&Uri> {
    match selector {
        Some(DatasetSelector::Id(uri)) => Some(uri),
        Some(DatasetSelector::Default) | None => None,
    }
}

/// Attribute collection append handling.
pub mod append_attributes;
/// Entity creation handling.
pub mod create;
/// Entity deletion handling.
pub mod delete;
/// Attribute deletion handling.
pub mod delete_attribute;
/// Delete-by-query handling.
pub mod delete_by_query;
/// Entity merge handling.
pub mod merge;
/// Requests missing a required entity identifier.
pub mod missing_identifier;
/// Partial attribute update handling.
pub mod partial_update_attribute;
/// Entity collection query handling.
pub mod query;
/// Entity replacement handling.
pub mod replace;
/// Attribute replacement handling.
pub mod replace_attribute;
/// Single-entity retrieval handling.
pub mod retrieve;
/// Attribute collection update handling.
pub mod update_attributes;
