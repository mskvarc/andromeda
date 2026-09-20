//! Subscription notification integration tests using the standard Rust harness.
//!
//! Naming convention: `notification_<feature>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

#[path = "../support/notification_adapter.rs"]
pub mod notification_adapter;

mod attribute_deleted;
mod basic;
mod delivery_after_commit;
mod helpers;
mod join;
mod mqtt;
mod show_changes;
