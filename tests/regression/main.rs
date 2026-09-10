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
mod http_binding_details;
mod list_attribute_families;
mod ordering_collation;
mod prior_findings;
mod query_language_evaluator;
mod query_language_targets;
mod temporal_aggregation;
