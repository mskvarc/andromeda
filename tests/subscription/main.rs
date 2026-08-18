//! Subscription API integration tests using the standard Rust harness.
//!
//! Naming convention: `subscription_<operation>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

#[path = "../support/notification_adapter.rs"]
pub mod notification_adapter;

mod create;
mod delete;
mod delivery_state;
mod helpers;
mod query;
mod retrieve;
mod update;
