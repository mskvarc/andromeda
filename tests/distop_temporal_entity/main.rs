//! Distributed temporal entity operations integration tests.
//!
//! Tests temporal query and retrieve distributed operations:
//! - Query: forwarding, CSR modes (inclusive/auxiliary/exclusive), filtering, merging
//! - Retrieve: local-only, attribute merging, error handling
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly.
//!
//! Naming convention: `distop_temporal_<operation>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod query;
mod retrieve;
