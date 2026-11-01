//! The dormant-tenant wake probe: every store answering for the work it owns.
//!
//! The context store answers first, for its queues, expiries and periodic
//! notifications, and so vouches for the tenant's schema before the temporal
//! store reads its history chunks for the conversion and retention sweeps. Each
//! reads over a transient request-path connection of its own.

use andromeda_configuration::{broker_config::BrokerConfig, schema::temporal::storage::StorageDaysError};
use andromeda_postgresql_context_store::pending_work::PostgresPendingWorkProbe;
use andromeda_postgresql_pool::database_url::DatabaseUrl;
use andromeda_postgresql_temporal_store::{
    history_due::HistorySchedule,
    history_work_probe::PostgresHistoryWorkProbe,
    retention_sweep::TemporalRetentionHorizonInvalid,
};
use andromeda_temporal::storage_policy::TemporalStoragePolicy;
use andromeda_tenancy::{pending_work::PendingWorkProbe, pending_work_sources::PendingWorkSources};
use std::sync::Arc;
use thiserror::Error;

/// `temporal.storage` names no usable sweep horizon, which configuration
/// validation refuses first.
#[derive(Debug, Error)]
pub(crate) enum PendingWorkProbeError {
    /// A `temporal.storage` horizon is not a positive whole number of days.
    #[error("invalid configuration: {0}")]
    TemporalStorage(#[from] StorageDaysError),
    /// The retention horizon reaches past the representable instants.
    #[error("invalid configuration: {0}")]
    TemporalRetention(#[from] TemporalRetentionHorizonInvalid),
}

/// The probe the wake loop asks for a dormant tenant's earliest due work.
///
/// # Errors
///
/// Returns [`PendingWorkProbeError`] when `temporal.storage` names no usable
/// sweep horizon.
pub(crate) fn pending_work_probe(request_url: &DatabaseUrl, config: &BrokerConfig) -> Result<Arc<dyn PendingWorkProbe>, PendingWorkProbeError> {
    let history = HistorySchedule::try_from(&TemporalStoragePolicy::try_from(&config.temporal.storage)?)?;
    Ok(Arc::new(PendingWorkSources::new(vec![
        Arc::new(PostgresPendingWorkProbe::new(
            request_url.clone(), // clone: each probe derives tenant URLs from its own copy of the request base
            config.database.acquire_timeout(),
        )),
        Arc::new(PostgresHistoryWorkProbe::new(
            request_url.clone(), // clone: each probe derives tenant URLs from its own copy of the request base
            config.database.acquire_timeout(),
            history,
        )),
    ])))
}
