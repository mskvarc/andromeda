//! Snapshot conformance and reliability acceptance suite.

#![allow(clippy::expect_used, reason = "integration test transport failures must stop the case")]

mod branch_schema;
mod capture;
mod core;
mod coverage;
mod deferred_history;
mod lifecycle;
mod model_protocol;
#[path = "../support/notification_adapter.rs"]
mod notification_adapter;
mod purge;
mod stress;
mod subscriptions;
mod support;
mod temporal;
mod tenancy_resilience;
