//! Shared infrastructure for Andromeda integration tests.
//!
//! Every `tests/<suite>/` binary depends on this crate and imports the
//! full kit via `use andromeda_test_common::prelude::*;`.
//!
//! Goals:
//! - single source of truth for `BASE_URL`, `DATABASE_URL`, canonical contexts,
//!   the HTTP client wrapper (`NgsiLdClient`), the `MockServer` Context Source
//!   stub, and the notification receivers.
//! - per-test isolation via [`crate::scope::TestScope`] — a tenant allocator and entity-ID
//!   prefix that enables parallel test execution without a global
//!   `TRUNCATE CASCADE` round trip between every test.
//! - JSON fixtures on disk (see `fixtures/`), loaded lazily and cached per
//!   process so the Rust sources contain only the semantic test logic.

pub mod assert;
pub mod client;
pub mod config;
pub mod db;
pub mod error;
pub mod fixture;
pub mod header;
pub mod ids;
pub mod mock;
pub mod prelude;
pub mod scope;
