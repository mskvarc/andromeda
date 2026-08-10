//! Distributed entity operations integration tests.
//!
//! Tests for distributed entity operations across context sources including:
//! - Entity retrieval with different registration modes (inclusive, exclusive, auxiliary, redirect)
//! - Entity queries with merging, entityMap, and splitEntities
//! - Entity provisioning (create, delete, merge, replace)
//! - Via header loop detection
//! - NGSILD-Warning headers for context source errors
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly.
//!
//! Naming convention: `distop_entity_<operation>_<mode>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

#[path = "../support/context_source_adapter.rs"]
pub mod context_source_adapter;

mod helpers;
mod provision;
mod query;
mod retrieve;
mod via_loop_detection;
mod warning_headers;
