//! Pagination Link header integration tests for the NGSI-LD context broker.
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly.
//!
//! Naming convention: `pagination_<endpoint>_<scenario>_<expected_outcome>`

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod helpers;
mod query;
