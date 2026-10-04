//! Subscription notification integration tests using the standard Rust harness.
//!
//! Naming convention: `notification_<feature>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

#[path = "../support/notification_adapter.rs"]
pub mod notification_adapter;

mod attribute_deleted;
mod basic;
mod delivery_after_commit;
mod expired_instances;
mod helpers;
mod join;
mod join_per_subscription;
mod mqtt;
mod notified_entity_members;
mod periodic_payload;
mod q_linked_traversal;
mod q_target_language;
mod show_changes;
