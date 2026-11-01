//! Answering the wake loop for the history a dormant tenant owes its sweeps.
//!
//! The conversion and retention sweeps run as tasks of a loaded tenant, so a
//! tenant no request loads would keep its aged chunks in the rowstore and its
//! expired history in storage. This probe reports when the tenant's history
//! next falls due ([`crate::history_due`]), and the wake loop loads the tenant
//! then, as it does for any other obligation. It reads over a transient
//! one-connection pool, so it adds no permanent backend and is safe through a
//! transaction pooler. The pool reaches an established tenant over its request
//! URL and a provisional one directly ([`TenantProbeUrls`]).

use crate::history_due::{HistorySchedule, earliest_history_due};
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_postgresql_pool::{pending_work_failure::pending_work_failure, pool_options::configure_pool_options, tenant_probe_urls::TenantProbeUrls};
use andromeda_tenancy::{
    pending_work::{PendingWork, PendingWorkError, PendingWorkProbe},
    tenant_existence::TenantExistence,
};
use async_trait::async_trait;
use std::time::{Duration, SystemTime};

/// Reads a tenant's database for the instant its history next owes a sweep.
pub struct PostgresHistoryWorkProbe {
    /// The bases every per-tenant probe connection is derived from.
    urls: TenantProbeUrls,
    /// How long the transient probe pool waits for a connection before it
    /// treats the backend as saturated.
    acquire_timeout: Duration,
    /// The horizons the tenant's sweeps run under.
    schedule: HistorySchedule,
}

impl PostgresHistoryWorkProbe {
    /// A probe over the given bases, acquire deadline and sweep horizons.
    #[must_use]
    pub const fn new(urls: TenantProbeUrls, acquire_timeout: Duration, schedule: HistorySchedule) -> PostgresHistoryWorkProbe {
        PostgresHistoryWorkProbe {
            urls,
            acquire_timeout,
            schedule,
        }
    }
}

#[async_trait]
impl PendingWorkProbe for PostgresHistoryWorkProbe {
    async fn earliest_pending_work(&self, database: &TenantDatabaseName, existence: TenantExistence) -> Result<PendingWork, PendingWorkError> {
        let url = self.urls.tenant_url(database, existence);
        let pool = configure_pool_options(1, self.acquire_timeout)
            .connect(url.as_str())
            .await
            .map_err(|error| pending_work_failure(&error, database))?;
        let due = match pool.acquire().await {
            Ok(mut connection) => earliest_history_due(&mut connection, self.schedule).await,
            Err(error) => Err(error),
        };
        pool.close().await;
        due.map(|due| due.map_or(PendingWork::None, |due| PendingWork::DueAt(SystemTime::from(due))))
            .map_err(|error| pending_work_failure(&error, database))
    }
}
