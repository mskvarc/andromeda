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
pub mod prelude;
pub mod scope;

#[cfg(test)]
mod boundary {
    /// The test kit stays a quarantine: it declares no in-workspace dependency
    /// it could ship, so nothing production reaches through it and it can never
    /// become a shared code path. The one edge it carries — the layer guard,
    /// which declares nothing of its own — is a dev-only edge.
    ///
    /// The third-party denylist passed here is deliberately empty: a test kit
    /// legitimately builds against the runtime, the transports and the driver,
    /// so the invariant this pins is the workspace quarantine, not the baseline
    /// every specification crate is held to.
    #[test]
    fn the_test_kit_declares_no_in_workspace_dependency_it_could_ship() {
        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), &[], &["andromeda-layer-guard"]);
        andromeda_layer_guard::manifest::assert_dev_only(env!("CARGO_MANIFEST_DIR"), "andromeda-layer-guard");
    }
}
