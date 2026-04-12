//! Subscription notification integration tests using the standard Rust harness.
//!
//! Naming convention: `notification_<feature>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod attribute_deleted;
mod basic;
mod helpers;
mod join;
mod mqtt;
mod show_changes;
