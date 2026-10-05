use crate::{
    error::{EntityResultExt, public_error},
    join::{JoinRequest, ResolvedJoin},
    request_projection::RequestProjection,
    state::{EntityTenantState, EntityTenantStateOrCreate},
};
use andromeda_entity::{merge_options::MergeOptions, query::retrieval::RetrievalNarrowing};
use andromeda_entity_map::contribution::SourceContribution;
use andromeda_federation::{
    entity_result::{collect_proxied_attrs, collect_warnings, contributions_of, local_remainder_of_payload},
    operation::{ForwardingOperation, ProvisionResource},
    provision_purge::{ForwardPurgeRequest, ForwardedPurgeResult},
    request::{CsrModeFilter, ForwardProvisionRequest, ForwardQueryRequest},
    single_source_failure::single_source_failure,
};
use andromeda_http_context_registry::response_rendering::ResponseContextSource;
use andromeda_http_extraction::{
    accept::AcceptFormat,
    context::ContextUrl,
    entity_query::EntityQuery,
    jsonld::{JsonLd, JsonLdDocument},
    protocol::NgsiLdProtocolHeaders,
    query::ValidatedQuery,
    request_envelope::NgsiLdRequestEnvelope,
    typed_path::TypedPath,
    via::IncomingVia,
};
use andromeda_http_response::{
    batch_projection::{batch_operation_result, forwarded_write_error},
    collection::NgsiLdCollectionResponse,
    entity_failure::classify_entity_error,
    forwarding_failure::classify_forwarding_error,
    linked_resolution_warning::with_linked_resolution_warnings,
    problem_response::ProblemResponse,
    provision::{LocalOutcome, multi_status, provision_response},
    single::NgsiLdResponse,
};
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_entity::operation::selection::{DatasetSelection, OverwritePolicy};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    entity::{Entity, EntityPatch},
    entity_type::EntityTypeValue,
};
use andromeda_ngsi_ld_problem_details::{batch_operation::BatchOperationResult, error_type::NgsiLdErrorType};
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, projection::purge::PurgeMode};
use andromeda_ngsi_ld_render::geometry_selection::GeometrySelection;
use andromeda_ngsi_ld_request::{
    attribute_deletion::DeleteAttrParams,
    entity::{AppendParams, GetEntityParams, MergeParams, PartialUpdateAttrParams, ReplaceAttrParams},
    local::LocalOnlyParams,
    options::OverwritePolicy as RequestOverwritePolicy,
};
use axum::{
    extract::OriginalUri,
    http::{StatusCode, Uri as HttpUri},
    response::{IntoResponse, Response},
};
use serde_json::Value;

const fn selected_dataset_uri(selector: Option<&DatasetSelector>) -> Option<&Identifier> {
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
/// The `EntityMap` a purge determines its entities from.
pub mod purge_entity_map;
/// Entity collection query handling.
pub mod query;
/// Admitting a decoded entity query before it runs.
pub mod query_admission;
/// Which HTTP binding carried a Query Entities request.
pub mod query_binding;
/// The forwarding phases of an entity query.
pub mod query_forwarding;
/// Where the page of an entity query is cut.
pub mod query_page;
/// Assembling the response an entity query answers with.
pub mod query_response;
/// The filters `splitEntities` decides after the merge.
pub mod query_split_filters;
/// Entity replacement handling.
pub mod replace;
/// Attribute replacement handling.
pub mod replace_attribute;
/// The `EntityMap` a distributed read runs under.
pub mod request_entity_map;
/// Single-entity retrieval handling.
pub mod retrieve;
/// The forwarding phases of a single-entity retrieval.
pub mod retrieve_forwarding;
/// Attribute collection update handling.
pub mod update_attributes;
/// The clause 5.2.18 `UpdateResult` an attribute write answers with.
pub mod update_result;
