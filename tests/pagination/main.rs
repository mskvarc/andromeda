//! Pagination Link header integration tests for the NGSI-LD context broker.
//!
//! Naming convention: `pagination_<endpoint>_<scenario>_<expected_outcome>`.
//!
//! These tests assert exact `limit`/`offset` semantics against broker-wide
//! subscription and registration collections, so every test runs under
//! `#[serial(global)]` and wipes broker state on entry.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod csource_subscription;
mod cursor_walk;
mod entity;
mod query;
mod registration;
mod subscription;
mod temporal;
