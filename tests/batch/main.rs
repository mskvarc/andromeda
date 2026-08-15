//! Batch API integration tests using the standard Rust harness.
//!
//! Naming convention: `batch_<operation>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod create;
mod delete;
mod helpers;
mod merge;
mod update;
mod upsert;
