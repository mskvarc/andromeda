//! The checks a loaded configuration passes before the broker reads it.
//!
//! Each table checks its own values; the cross-section rules check what no
//! one table can see. A check that needs a domain type belongs to the crate
//! that owns the type, and the broker composition runs it after these.

mod cross_section;

use crate::{
    broker_config::BrokerConfig,
    schema::{
        database::DatabaseConfigError,
        entity::EntityConfigError,
        expiry::ExpiryConfigError,
        federation::FederationConfigError,
        http::HttpStackConfigError,
        jsonld::JsonLdConfigError,
        management::ManagementConfigError,
        notifications::NotificationsConfigError,
        request_limits::RequestLimitsConfigError,
        server::ServerConfigError,
        snapshot::SnapshotConfigError,
        temporal::error::TemporalConfigError,
        tenancy::TenancyConfigError,
        tls::TlsConfigError,
    },
    validate::cross_section::{
        check_acquire_timeout_below_request_timeout,
        check_build_admission_below_request_timeout,
        check_management_port_distinct_from_server,
        check_metrics_have_a_management_plane,
        check_side_effect_lease_over_acquire_timeout,
        check_tls_material_for_listeners,
    },
};
use humantime::format_duration;
use std::time::Duration;
use thiserror::Error;

/// A loaded configuration a check refused.
#[derive(Debug, Error)]
pub enum InvalidConfiguration {
    /// The `[server]` table violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Server(#[from] ServerConfigError),
    /// The `[management]` table violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Management(#[from] ManagementConfigError),
    /// The `[request_limits]` table violated its declared invariant.
    #[error("invalid configuration: {0}")]
    RequestLimits(#[from] RequestLimitsConfigError),
    /// The `[http]` table violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Http(#[from] HttpStackConfigError),
    /// The `[database]` table violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Database(#[from] DatabaseConfigError),
    /// The `[tenancy]` table violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Tenancy(#[from] TenancyConfigError),
    /// The `[expiry]` table violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Expiry(#[from] ExpiryConfigError),
    /// The `[jsonld]` table violated its declared invariant.
    #[error("invalid configuration: {0}")]
    JsonLd(#[from] JsonLdConfigError),
    /// The `[entity]` table violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Entity(#[from] EntityConfigError),
    /// The `[temporal]` table violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Temporal(#[from] TemporalConfigError),
    /// The `[notifications]` table violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Notifications(#[from] NotificationsConfigError),
    /// The `[federation]` table violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Federation(#[from] FederationConfigError),
    /// The `[snapshot]` table violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Snapshot(#[from] SnapshotConfigError),
    /// A listener that terminates TLS has no usable `[tls]` identity.
    #[error("invalid configuration: {0}")]
    Tls(#[from] TlsConfigError),
    /// The connection acquire deadline is not below the request timeout.
    #[error(
        "invalid configuration: database.acquire_timeout ({}) must be below request_limits.timeout ({}) so a saturated pool answers 503 before the request deadline",
        format_duration(*acquire),
        format_duration(*request)
    )]
    AcquireTimeoutNotBelowRequestTimeout {
        /// The configured acquire deadline.
        acquire: Duration,
        /// The configured request timeout.
        request: Duration,
    },
    /// The tenant cold-build admission deadline is not below the request
    /// timeout.
    #[error(
        "invalid configuration: tenancy.build.admission_timeout ({}) must be below request_limits.timeout ({}) so a request that cannot be admitted to a cold build answers 503 before the request deadline",
        format_duration(*admission),
        format_duration(*request)
    )]
    BuildAdmissionNotBelowRequestTimeout {
        /// The configured admission deadline.
        admission: Duration,
        /// The configured request timeout.
        request: Duration,
    },
}

/// Run every table's own checks, then the cross-section rules.
///
/// # Errors
///
/// Returns the [`InvalidConfiguration`] of the first check that refuses.
pub fn validate(config: &BrokerConfig) -> Result<(), InvalidConfiguration> {
    config.server.validate()?;
    config.management.validate()?;
    config.request_limits.validate()?;
    config.http.validate()?;
    config.database.validate()?;
    config.tenancy.validate()?;
    config.expiry.validate()?;
    config.jsonld.validate()?;
    config.entity.validate()?;
    config.temporal.validate()?;
    config.notifications.validate()?;
    config.federation.validate()?;
    config.snapshot.validate()?;
    check_acquire_timeout_below_request_timeout(config)?;
    check_side_effect_lease_over_acquire_timeout(config)?;
    check_build_admission_below_request_timeout(config)?;
    check_management_port_distinct_from_server(config)?;
    check_metrics_have_a_management_plane(config)?;
    check_tls_material_for_listeners(config)?;
    Ok(())
}
