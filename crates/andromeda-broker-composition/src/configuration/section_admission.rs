//! Admitting each loaded table into the domain values it is read as.
//!
//! A table's own values are checked where the table is defined; a value
//! whose rules belong to a domain type is checked by converting it into that
//! type, which only the composition, seeing every domain crate, can do. Every
//! conversion runs once while the configuration loads, so a broker never
//! starts with a value a domain would refuse later. A cap a domain type holds
//! that one comparison states is declared by the configuration instead, so
//! its descriptor carries it, and the domain crate's tests pin the declared
//! cap to its own. The derivations that read more than one table live here
//! too.

use crate::configuration::source_identity::{SourceIdentity, SourceIdentityError};
use andromeda_configuration::broker_config::BrokerConfig;
use andromeda_entity::configured_policy::side_effect_retry_schedule;
use andromeda_http_stack::{
    cors_tokens::{CorsTokenError, check_cors_tokens},
    stack::helmet_layer::{SecurityHeadersError, check_security_headers},
};
use andromeda_postgresql_context_store::database_defaults::{DatabaseDefaults, StatementTimeout};
use andromeda_postgresql_maintenance::expected_deployment::{ColumnstoreConversion, ExpectedDeployment, HistoryRetention};
use andromeda_retry::schedule_error::BackoffScheduleError;
use andromeda_temporal::configured_policy::intent_retry_schedule;
use thiserror::Error;

/// A loaded table holds a value its domain refuses.
#[derive(Debug, Error)]
pub(crate) enum SectionAdmissionError {
    /// A CORS method or header name is not one the HTTP layer reads.
    #[error("invalid configuration: {0}")]
    HttpCors(#[from] CorsTokenError),
    /// A security response field holds a value no field can carry.
    #[error("invalid configuration: {0}")]
    HttpSecurityHeaders(#[from] SecurityHeadersError),
    /// The temporal history queue's retry delays do not bound a retry
    /// schedule: the first is zero or the ceiling is below it.
    #[error("invalid configuration: temporal.history.queue.retry_interval and max_retry_interval do not form a retry schedule: {0}")]
    TemporalIntentRetry(#[source] BackoffScheduleError),
    /// The side-effect queue's retry delays do not bound a retry schedule.
    #[error("invalid configuration: entity.side_effects.queue.retry_interval and max_retry_interval do not form a retry schedule: {0}")]
    EntitySideEffectRetry(#[source] BackoffScheduleError),
    /// The identity in effect is not one the broker can report: no alias can
    /// be found, the alias is no `Via` token, the identifier is no
    /// identifier, or the extras are no admissible JSON.
    #[error("invalid configuration: {0}")]
    SourceIdentity(#[from] SourceIdentityError),
}

/// Convert every table value whose rules belong to a domain type.
///
/// # Errors
///
/// Returns the [`SectionAdmissionError`] of the first value its domain refuses.
pub(crate) fn admit_sections(config: &BrokerConfig) -> Result<(), SectionAdmissionError> {
    check_cors_tokens(&config.http.cors)?;
    check_security_headers(&config.http.security_headers)?;
    intent_retry_schedule(&config.temporal.history).map_err(SectionAdmissionError::TemporalIntentRetry)?;
    side_effect_retry_schedule(&config.entity.side_effects).map_err(SectionAdmissionError::EntitySideEffectRetry)?;
    SourceIdentity::resolve(&config.federation.identity, config.server.public_url.as_ref(), machine_host_name)?;
    Ok(())
}

/// The machine's host name, the last place a left-out alias is taken from.
pub(crate) fn machine_host_name() -> Option<String> {
    whoami::hostname().ok()
}

/// The session defaults installed on every database the broker serves from:
/// the configured commit durability, and a statement timeout just past the
/// request deadline.
#[must_use]
pub(crate) const fn database_defaults(config: &BrokerConfig) -> DatabaseDefaults {
    DatabaseDefaults {
        synchronous_commit: config.database.synchronous_commit,
        statement_timeout: StatementTimeout::past_request_deadline(config.request_limits.timeout.duration()),
    }
}

/// What the maintenance controller checks the cluster against: the prefix of
/// every tenant database name, and how temporal history is converted to the
/// columnstore and aged out.
#[must_use]
pub(crate) fn expected_deployment(config: &BrokerConfig) -> ExpectedDeployment {
    let columnstore = &config.temporal.storage.columnstore;
    ExpectedDeployment {
        tenant_database_prefix: config.tenancy.databases.name_prefix.clone(), // clone: the controller keeps its own copy of the prefix
        columnstore_conversion: ColumnstoreConversion {
            after: columnstore.convert_after.duration(),
            sweep_interval: columnstore.sweep_interval.duration(),
        },
        history_retention: config
            .temporal
            .storage
            .retention_age()
            .map_or(HistoryRetention::Disabled, |delete_after| HistoryRetention::Enabled {
                delete_after: delete_after.duration(),
                sweep_interval: config.temporal.storage.retention.sweep_interval.duration(),
            }),
    }
}

#[cfg(test)]
mod tests;
