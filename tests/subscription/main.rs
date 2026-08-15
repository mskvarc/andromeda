//! Subscription API integration tests using the standard Rust harness.
//!
//! Naming convention: `subscription_<operation>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod create;
mod delete;
mod helpers;
mod query;
mod retrieve;
mod update;
