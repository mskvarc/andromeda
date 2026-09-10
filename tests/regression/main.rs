//! Regression tests for defects that do not belong to any feature suite.
//!
//! Each module owns one defect family and reproduces it through the HTTP API,
//! asserting the exact behaviour ETSI GS CIM 009 v1.9.1 mandates rather than
//! the behaviour the broker happened to have when the defect was found.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod attribute_member_validation;
mod batch_upsert_duplicates;
mod context_cache_expiry;
mod entity_map_pruning;
mod geojson_geometry_selection;
mod http_binding_details;
mod http_precondition_details;
mod list_attribute_families;
mod list_relationship_representation;
mod null_sentinel_admission;
mod operation_group_expansion;
mod ordering_collation;
mod prior_findings;
mod query_language_evaluator;
mod query_language_targets;
mod scope_null_sentinel;
mod subscription_endpoint_members;
mod temporal_aggregation;
mod temporal_collation;
mod temporal_entity_map;
mod temporal_evolution_delete_order;
mod time_value_restrictions;

#[path = "../support/context_source_adapter.rs"]
pub mod context_source_adapter;
