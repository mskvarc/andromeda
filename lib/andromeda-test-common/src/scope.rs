//! Per-test isolation via id-prefix scoping.
//!
//! Each integration test allocates a [`TestScope`] at entry; the scope owns
//! a unique URN prefix and, optionally, a tenant header. Every id minted
//! through the scope inherits the prefix, so [`TestScope::cleanup`] can
//! wipe the scope's rows via a single `id LIKE prefix%` batch without
//! touching other parallel tests' data.
//!
//! Cleanup is run explicitly — call `scope.cleanup().await` at the end of
//! the test body. A `Drop` impl would require blocking on Tokio from a
//! synchronous context, which Tokio rejects.

use crate::{db, ids};

/// Scope metadata carried into every request by [`crate::client::NgsiLdClient`].
#[derive(Debug, Clone)]
pub struct TestScope {
    suite: String,
    id_prefix: String,
    tenant: Option<String>,
}

impl TestScope {
    /// Allocate a new scope for the `suite` identifier (e.g. `"entity"`,
    /// `"subscription"`). The id prefix is globally unique; no two parallel
    /// tests can collide on it.
    pub fn new(suite: &str) -> TestScope {
        TestScope {
            suite: suite.to_owned(),
            id_prefix: ids::prefix(suite),
            tenant: None,
        }
    }

    /// Construct the scope reserved for `#[serial(global)]` tests.
    ///
    /// Returns a scope with an empty id prefix — callers must not mint ids
    /// through [`TestScope::id`] in this mode; they call [`TestScope::global_truncate`]
    /// instead to start from a deterministically empty broker.
    pub fn global(suite: &str) -> TestScope {
        TestScope {
            suite: suite.to_owned(),
            id_prefix: String::new(),
            tenant: None,
        }
    }

    /// Attach an `NGSILD-Tenant` header to every request issued through the
    /// scope's client. The named tenant is provisioned lazily by the broker
    /// on first use.
    pub fn with_tenant(mut self, tenant: impl Into<String>) -> TestScope {
        self.tenant = Some(tenant.into());
        self
    }

    /// Suite name supplied to [`TestScope::new`].
    pub fn suite(&self) -> &str {
        &self.suite
    }

    /// Scope's id prefix — every minted id starts with this string.
    pub fn id_prefix(&self) -> &str {
        &self.id_prefix
    }

    /// Tenant name, if any.
    pub fn tenant(&self) -> Option<&str> {
        self.tenant.as_deref()
    }

    /// Compose a scoped id by appending `local` to the scope's prefix.
    ///
    /// `local` is typically the entity-type label (`"Building"`, `"Vehicle"`)
    /// plus a discriminator when the test needs multiple instances of the
    /// same type.
    pub fn id(&self, local: &str) -> String {
        ids::compose(&self.id_prefix, local)
    }

    /// Delete every row this scope created from the default tenant database.
    ///
    /// See [`crate::db::delete_by_prefix`] for the exact statements issued.
    pub async fn cleanup(&self) -> Result<(), sqlx::Error> {
        if self.id_prefix.is_empty() {
            return Ok(());
        }
        db::delete_by_prefix(&self.id_prefix).await
    }

    /// Full `TRUNCATE CASCADE` of every tenant-scoped table. Reserved for
    /// `#[serial(global)]` tests that depend on broker-wide state.
    pub async fn global_truncate(&self) -> Result<(), sqlx::Error> {
        db::truncate_all().await
    }
}
