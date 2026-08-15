//! NGSI-LD context broker info integration tests.
//!
//! Naming convention: `info_<operation>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod component_report;
mod health;
mod removed_routes;
mod source_identity;
