//! Distributed temporal entity operations integration tests.
//!
//! Tests temporal splitEntities post-merge filtering.
//! Standalone test suite using `tokio::test` and `reqwest` directly.
//!
//! Naming convention: `distop_temporal_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod query;
