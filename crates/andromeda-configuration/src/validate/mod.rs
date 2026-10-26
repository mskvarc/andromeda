//! The checks a loaded configuration passes before the broker reads it.
//!
//! Each section checks its own values; the cross-section rules check what no
//! one section can see. A check that needs a domain type belongs to the crate
//! that owns the type, and the broker composition runs it after these.

mod cross_section;

use crate::{
    broker_config::BrokerConfig,
    schema::{
        database::DatabaseConfigError,
        entity::EntityConfigError,
        extensions::ExtensionsConfigError,
        federation::FederationConfigError,
        http::HttpStackConfigError,
        jsonld::JsonLdConfigError,
        management::ManagementConfigError,
        notifications::NotificationsConfigError,
        server::ServerConfigError,
        snapshot::SnapshotConfigError,
        spatial::SpatialConfigError,
        temporal::error::TemporalConfigError,
        tenancy::TenancyConfigError,
        tls::TlsConfigError,
    },
    validate::cross_section::{
        check_acquire_timeout_below_request_timeout,
        check_build_admission_below_request_timeout,
        check_management_port_distinct_from_server,
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
    /// The `[server]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Server(#[from] ServerConfigError),
    /// The `[http]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Http(#[from] HttpStackConfigError),
    /// The `[database]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Database(#[from] DatabaseConfigError),
    /// The `[jsonld]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    JsonLd(#[from] JsonLdConfigError),
    /// The `[temporal]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Temporal(#[from] TemporalConfigError),
    /// The `[tenancy]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Tenancy(#[from] TenancyConfigError),
    /// The `[entity]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Entity(#[from] EntityConfigError),
    /// The `[federation]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Federation(#[from] FederationConfigError),
    /// The `[snapshot]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Snapshot(#[from] SnapshotConfigError),
    /// The `[notifications]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Notifications(#[from] NotificationsConfigError),
    /// The `[spatial]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Spatial(#[from] SpatialConfigError),
    /// The `[management]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Management(#[from] ManagementConfigError),
    /// The `[extensions]` section violated its declared invariant.
    #[error("invalid configuration: {0}")]
    Extensions(#[from] ExtensionsConfigError),
    /// A listener that terminates TLS has no usable `[tls]` identity.
    #[error("invalid configuration: {0}")]
    Tls(#[from] TlsConfigError),
    /// The connection acquire deadline is not below the HTTP request timeout.
    #[error(
        "invalid configuration: database.acquire_timeout ({}) must be below http.request_timeout ({}) so a saturated pool answers 503 before the request deadline",
        format_duration(*acquire),
        format_duration(*request)
    )]
    AcquireTimeoutNotBelowRequestTimeout {
        /// The configured acquire deadline.
        acquire: Duration,
        /// The configured request timeout.
        request: Duration,
    },
    /// The tenant cold-build admission deadline is not below the HTTP request
    /// timeout.
    #[error(
        "invalid configuration: tenancy.build_admission_timeout ({}) must be below http.request_timeout ({}) so a request that cannot be admitted to a cold build answers 503 before the request deadline",
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

/// Run every section's own checks, then the cross-section rules.
///
/// # Errors
///
/// Returns the [`InvalidConfiguration`] of the first check that refuses.
pub fn validate(config: &BrokerConfig) -> Result<(), InvalidConfiguration> {
    config.server.validate()?;
    config.http.validate()?;
    config.database.validate()?;
    config.jsonld.validate()?;
    config.temporal.validate()?;
    config.tenancy.validate()?;
    config.entity.validate()?;
    config.federation.validate()?;
    config.snapshot.validate()?;
    config.notifications.validate()?;
    config.spatial.validate()?;
    config.management.validate()?;
    config.extensions.validate()?;
    check_acquire_timeout_below_request_timeout(config)?;
    check_side_effect_lease_over_acquire_timeout(config)?;
    check_build_admission_below_request_timeout(config)?;
    check_management_port_distinct_from_server(config)?;
    check_tls_material_for_listeners(config)?;
    Ok(())
}
