//! Centralized Axum request extraction for Andromeda's HTTP bindings.
//!
//! Every `FromRequest` and `FromRequestParts` implementation belongs here, including endpoint-specific body and query extractors and generic focused tenant-state extraction. Extractors may decode policy input types but never invoke policy services or select operations.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod accept;
pub mod accept_language;
pub mod addressed_uri;
pub mod admitted_temporal_query;
pub mod api_root;
pub mod api_root_scheme;
mod batch_array;
pub mod batch_body_refusal;
pub mod batch_item_decoding;
pub mod batch_item_refusal;
pub mod blocking_decode;
mod body_admission;
mod body_admission_refusal;
mod body_envelope;
pub mod body_only_query;
pub mod body_preconditions;
pub mod classification;
mod content_kind;
pub mod content_length_precondition;
pub mod context;
pub mod context_target;
pub mod entity_batch;
pub mod entity_id_batch;
pub mod entity_post_query;
pub mod entity_query;
mod forwarded_element;
pub mod forwarded_field_trust;
pub mod forwarded_host;
pub mod forwarded_protocol;
pub mod json_payload;
pub mod json_value_budget;
pub mod jsonld;
pub mod language;
pub mod listener_scheme;
pub mod not_json_document;
pub mod notification_body;
pub mod operation_hit_scope;
pub mod pagination_policy;
pub mod path_segment;
pub mod prefer;
pub mod profile;
pub mod protocol;
pub mod query;
pub mod query_decoding;
pub mod query_refusal;
pub mod request_body_limit;
pub mod request_envelope;
pub mod request_geometry_position_limit;
pub mod request_json_value_limit;
pub mod request_links;
pub mod request_payload;
pub mod resource_location;
pub mod response_context_request;
pub mod scoped_response;
pub mod snapshot;
pub mod snapshot_purge;
pub mod snapshot_scope_guard;
pub mod submitted_body;
pub mod temporal_delete_query;
pub mod tenant;
pub mod tenant_state;
pub mod typed_path;
pub mod undecodable_body;
pub mod via;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration",
        "andromeda-context-space",
        "andromeda-http-headers",
        "andromeda-http-response",
        "andromeda-json-literal",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-request",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-persistence-failure",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["axum", "tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
