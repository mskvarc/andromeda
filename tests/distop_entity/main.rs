//! Distributed entity operations integration tests.
//!
//! Tests entityMap with forwarded IDs and splitEntities post-merge filtering.
//! Standalone test suite using `tokio::test` and `reqwest` directly.
//!
//! Naming convention: `distop_entity_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod query;
