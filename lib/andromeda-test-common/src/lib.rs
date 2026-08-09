//! Shared infrastructure for Andromeda integration tests.
//!
//! Every `tests/<suite>/` binary depends on this crate and imports each test
//! capability from the module that defines it.
//!
//! Goals:
//! - single source of truth for `BASE_URL`, `DATABASE_URL`, canonical contexts,
//!   the HTTP client wrapper (`NgsiLdClient`), the `MockServer` Context Source
//!   stub, and the notification receivers.
//! - per-test isolation via [`crate::scope::TestScope`] — a tenant allocator and entity-ID
//!   prefix that enables parallel test execution without a global
//!   `TRUNCATE CASCADE` round trip between every test.
//! - a JSON-fixture loader (see [`crate::fixture`]) that reads payloads from
//!   `fixtures/` lazily and caches them per process; the tree is available but
//!   currently unpopulated, so suites still inline their payloads today.

#![deny(missing_docs)]

pub mod assert;
pub mod client;
pub mod config;
pub mod db;
pub mod error;
pub mod fixture;
pub mod header;
pub mod ids;
pub mod mock;
pub mod scope;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: all,
}
