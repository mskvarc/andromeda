use crate::{
    entity_error::classify_entity_error,
    error::{EntityResultExt, public_error},
    join::{JoinRequest, ResolvedJoin},
    request_projection::RequestProjection,
    state::{EntityTenantState, EntityTenantStateOrCreate},
};
use andromeda_entity::{merge_options::MergeOptions, query::retrieval::RetrievalNarrowing};
use andromeda_federation::{
    entity_result::{collect_proxied_attrs, collect_warnings, contributions_of, strip_attributes_from_value},
    operation::{ForwardingOperation, ProvisionResource},
    outcome::SourceContribution,
    provision::single_source_failure,
    provision_purge::{ForwardPurgeRequest, ForwardedPurgeResult},
    request::{CsrModeFilter, ForwardProvisionRequest, ForwardQueryRequest},
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
    batch_projection::{batch_operation_result, forwarded_write_error},
    classified_report::ClassifiedReport,
    collection::NgsiLdCollectionResponse,
    linked_resolution_warning::with_linked_resolution_warnings,
    problem_response::ProblemResponse,
    provision::{LocalOutcome, classify_forwarding_error, multi_status, provision_response},
    render::start_rendering,
    single::NgsiLdResponse,
};
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    entity::{Entity, EntityPatch},
    entity_type::EntityTypeValue,
    expanded::mutation::selection::{DatasetSelection, OverwritePolicy},
};
use andromeda_ngsi_ld_problem_details::{batch_operation::BatchOperationResult, error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, filter::expansion::expand_q_expr, projection::purge::PurgeMode};
use andromeda_ngsi_ld_render::response::ResponsePayload;
use andromeda_ngsi_ld_representation::attribute_type_validation::{validate_attribute_node, validate_fragment_attribute_types};
use andromeda_ngsi_ld_request::{
    attribute_deletion::DeleteAttrParams,
    entity::{AppendParams, GetEntityParams, MergeParams, PartialUpdateAttrParams, ReplaceAttrParams},
    join_traversal::RetrievingJoin,
    linked_retrieval_consistency::reject_linked_retrieval,
    local::LocalOnlyParams,
    options::OverwritePolicy as RequestOverwritePolicy,
};
use axum::{
    Json,
    extract::OriginalUri,
    http::{StatusCode, Uri as HttpUri},
    response::{IntoResponse, Response},
};
use serde_json::Value;

/// Reject an entity fragment whose attribute nodes declare a `type` outside
/// clause 5.2.4 (clause 5.5.4: data not syntactically correct according to
/// the API data type definitions is `BadRequestData`).
fn reject_unknown_attribute_types(fragment: &Value, instance: &HttpUri) -> Result<(), ProblemResponse> {
    validate_fragment_attribute_types(fragment).map_err(|error| public_error(NgsiLdErrorType::BadRequestData, error.to_string(), instance))
}

/// Reject a single attribute node (clauses 5.6.4 and 5.6.19) whose `type` is
/// outside clause 5.2.4.
fn reject_unknown_attribute_node_type(attribute: &AttributeName, node: &Value, instance: &HttpUri) -> Result<(), ProblemResponse> {
    validate_attribute_node(attribute.as_str(), node).map_err(|error| public_error(NgsiLdErrorType::BadRequestData, error.to_string(), instance))
}

/// The response a read gives when the one exclusive or redirect source
/// holding all of its data declined the representation with `status` (Table
/// 6.4.3.2-3, clause 6.3.7): the source's 501 is the operation's answer.
pub(crate) fn single_source_declined(status: u16, instance: &HttpUri) -> ProblemResponse {
    ClassifiedReport::public_binding(
        ProblemClassification::NotImplemented,
        format!("the registered context source holding the requested data answered HTTP {status}: it does not support the requested representation"),
    )
    .with_instance(instance.clone())
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
/// The forwarding phases of an entity query.
pub mod query_forwarding;
/// Where the page of an entity query is cut.
pub mod query_page;
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
