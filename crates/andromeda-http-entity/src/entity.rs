use crate::{
    error::{EntityResultExt, internal_error, public_error},
    join::{JoinRequest, ResolvedJoin},
    provision_forwarding::{ForwardingModes, LocalOutcome, provision_response},
    request_projection::RequestProjection,
    state::{EntityTenantState, EntityTenantStateOrCreate},
};
use andromeda_entity::query::{EntityQueryService, retrieval::RetrievalNarrowing};
use andromeda_federation::{
    entity_result::{collect_proxied_attrs, collect_warnings, merge_retrieve_results, strip_attributes_from_value},
    operation::ForwardingOperation,
    outcome::ForwardedEntities,
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
    batch_projection::batch_operation_result,
    collection::NgsiLdCollectionResponse,
    linked_resolution_warning::with_linked_resolution_warnings,
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
use andromeda_ngsi_ld_request::{
    attribute_deletion::DeleteAttrParams,
    deferred_split_filters::DeferredSplitFilters,
    entity::{AppendParams, GetEntityParams, PartialUpdateAttrParams, ReplaceAttrParams},
    join_traversal::RetrievingJoin,
    linked_retrieval_consistency::reject_linked_retrieval,
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

/// Which registration returned which entities, for the `entityMap` member of
/// a created map (clause 5.2.39). The entry carrying only a loop warning names
/// no registration and contributes nothing.
pub(crate) fn remote_sources_of(forwarded: &[ForwardedEntities]) -> impl Iterator<Item = (Uri, Vec<Uri>)> + '_ {
    forwarded.iter().filter_map(|result| {
        let registration = result.registration_id.clone()?; // clone: the map owns the registration it attributes entities to
        Some((registration, result.entities.iter().map(|entity| entity.id.clone()).collect())) // clone: same
    })
}

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
