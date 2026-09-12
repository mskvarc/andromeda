//! Regression tests for defects that do not belong to any feature suite.
//!
//! Each module owns one defect family and reproduces it through the HTTP API,
//! asserting the exact behaviour ETSI GS CIM 009 v1.9.1 mandates rather than
//! the behaviour the broker happened to have when the defect was found.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod array_valued_property_history;
mod attribute_discovery_type_spelling;
mod attribute_member_validation;
mod attrs_scope_admission;
mod batch_context_expansion;
mod batch_upsert_duplicates;
mod coerced_term_representation;
mod context_cache_expiry;
mod context_source_alias_tenancy;
mod context_source_subscription_members;
mod csource_subscription_notification;
mod discovery_name_compaction;
mod embedded_user_context;
mod entity_map_pruning;
mod entity_map_scoping;
mod entity_map_source_declined;
mod forwarded_protocol_origin;
mod geojson_geometry_selection;
mod hosted_context_lifecycle;
mod http_binding_details;
mod http_precondition_details;
mod inline_join_multi_target_relationship;
mod invalid_parameter_values;
mod list_attribute_families;
mod list_relationship_representation;
mod minimal_query_parameters;
mod notification_representation;
mod null_sentinel_admission;
mod null_sentinel_deletion;
mod operation_group_expansion;
mod ordering_attribute_paths;
mod ordering_collation;
mod prefer_version_representation;
mod prior_findings;
mod projection_linked_entities;
mod projection_selector_exclusivity;
mod proof_seal_representation;
mod query_language_evaluator;
mod query_language_targets;
mod query_pattern_dialect;
mod query_string_escapes;
mod registration_discovery_information;
mod registration_health_members;
mod scope_null_sentinel;
mod scoped_user_context;
mod subscription_endpoint_members;
mod subscription_ngsild_conformance;
mod temporal_aggregation;
mod temporal_collation;
mod temporal_entity_map;
mod temporal_evolution_delete_order;
mod temporal_prefer_version;
mod temporal_scope_evolution;
mod term_declaration_prefix_resolution;
mod time_value_restrictions;
mod tls_terminator_deployment;
mod transient_entity_conflict;
mod unit_code_family_admission;
mod uri_comparison;

#[path = "../support/context_source_adapter.rs"]
pub mod context_source_adapter;
#[path = "../support/hosted_context.rs"]
pub mod hosted_context;
#[path = "../support/notification_adapter.rs"]
pub mod notification_adapter;
#[path = "../support/probe_entity_type.rs"]
pub mod probe_entity_type;
