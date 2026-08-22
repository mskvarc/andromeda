//! Full federation acceptance suite against the five-broker compose topology.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration tests use explicit setup assertions")]

#[path = "../support/notification_adapter.rs"]
mod notification_adapter;

mod architecture;
mod fixtures;
mod provision;
mod stress;
mod subscription;
mod temporal;
mod tenancy;
