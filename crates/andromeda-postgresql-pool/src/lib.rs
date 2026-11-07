//! Shared PostgreSQL connection-pool tuning and driver-error classification.
//!
//! Every pool the broker opens — the main request pool, per-tenant serving
//! pools, and the one-connection migration and maintenance pools — is built
//! from [`configure_pool_options`](crate::pool_options::configure_pool_options)
//! so durability-neutral tuning (idle reclaim, no warm minimum, the acquire
//! deadline) is decided once rather than per call site, and so is the share of
//! a tenant pool read requests may hold beside its writes and how a read runs a
//! statement as its planning asks. The crate also decides which driver errors
//! are retryable store conditions (saturation, a rolled back transaction, an
//! unreachable store) rather than faults, how a tenant database's URL is
//! derived from a base URL, which base a pending-work probe of a tenant
//! connects over, and how a probe's driver error reaches the wake loop. It is a
//! leaf below the context and temporal stores: it knows `sqlx`, `tokio`, the
//! statement trees both stores build and the tenancy port both stores answer,
//! and no store, so both stores can share it without either depending on the
//! other.

/// Returning a connection to its pool after its request let go of it.
pub mod connection_release;
/// The base URL every tenant database connection is derived from.
pub mod database_url;
/// A database error the tests can construct without a live driver connection.
#[cfg(any(test, feature = "test-support"))]
pub mod fake_database_error;
/// How a driver error a pending-work probe hit reaches the wake loop.
pub mod pending_work_failure;
/// How a `sqlx` error is classified into the failure a port carries.
pub mod persistence_classification;
/// The broker's shared pool tuning.
pub mod pool_options;
/// The connections of a tenant pool read requests may hold.
pub mod read_share;
/// Whether a driver error is the pool or the pooler at capacity.
pub mod saturation;
/// How a failure met while changing a schema is classified.
pub mod schema_change_classification;
/// Bounded waiting on the locks a schema change takes.
pub mod schema_change_lock;
/// The SQLSTATE codes the broker tells apart.
pub mod sql_state;
/// Whether a driver error says the store cannot be reached right now.
pub mod store_unavailability;
/// Which base a transient probe of a tenant's database connects over.
pub mod tenant_probe_urls;
/// Whether a driver error is a transaction rolled back under contention.
pub mod transaction_rollback;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-persistence-failure", "andromeda-postgresql-seaquery",
        "andromeda-retry",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["sea-query", "sqlx", "tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
