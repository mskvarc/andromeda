//! Admitting each loaded table into the domain values it is read as.
//!
//! A table's own values are checked where the table is defined; a value
//! whose rules belong to a domain type is checked by converting it into that
//! type, which only the composition, seeing every domain crate, can do. Every
//! conversion runs once while the configuration loads, so a broker never
//! starts with a value a domain would refuse later. The derivations that read
//! more than one table live here too.

use andromeda_configuration::broker_config::BrokerConfig;
use andromeda_entity::configured_policy::{
    AttributeNestingAboveDecodable,
    SideEffectAttemptsAboveCeiling,
    check_attribute_nesting,
    side_effect_max_attempts,
    side_effect_retry_schedule,
};
use andromeda_federation::cooldown_policy::cooldown_schedule;
use andromeda_http_stack::cors_tokens::{CorsTokenError, check_cors_tokens};
use andromeda_json_literal::{JsonLiteral, configured_extras::ConfiguredExtrasError};
use andromeda_ngsi_ld_lexicon::{
    context_source_pseudonym::{ContextSourcePseudonym, ContextSourcePseudonymError},
    identifier::{Identifier, IdentifierParseError},
};
use andromeda_notification::delivery_policy::{DeliveryPolicyError, check_delivery_policy};
use andromeda_postgresql_context_store::database_defaults::{DatabaseDefaults, StatementTimeout};
use andromeda_postgresql_maintenance::expected_deployment::{ColumnstoreConversion, ExpectedDeployment};
use andromeda_retry::schedule_error::BackoffScheduleError;
use andromeda_temporal::configured_policy::intent_retry_schedule;
use thiserror::Error;

/// A loaded table holds a value its domain refuses.
#[derive(Debug, Error)]
pub enum SectionAdmissionError {
    /// A CORS method or header name is not one the HTTP layer reads.
    #[error("invalid configuration: {0}")]
    HttpCors(#[from] CorsTokenError),
    /// The temporal history queue's retry delays do not bound a retry
    /// schedule: the first is zero or the ceiling is below it.
    #[error("invalid configuration: temporal.history.queue.retry_interval and max_retry_interval do not form a retry schedule: {0}")]
    TemporalIntentRetry(#[source] BackoffScheduleError),
    /// `request_limits.max_attribute_nesting` passes the depth every stored
    /// value reads back at.
    #[error("invalid configuration: {0}")]
    AttributeNesting(#[from] AttributeNestingAboveDecodable),
    /// The side-effect queue's retry delays do not bound a retry schedule.
    #[error("invalid configuration: entity.side_effects.queue.retry_interval and max_retry_interval do not form a retry schedule: {0}")]
    EntitySideEffectRetry(#[source] BackoffScheduleError),
    /// The side-effect queue's attempt ceiling is above what a job counts.
    #[error("invalid configuration: {0}")]
    EntitySideEffectAttempts(#[from] SideEffectAttemptsAboveCeiling),
    /// A cooldown that holds sources back needs a positive window, and an
    /// exponential one a cap no shorter than its first window.
    #[error(
        "invalid configuration: federation.cooldown.retry_interval and max_retry_interval do not form a cooldown schedule unless federation.cooldown.mode is disabled: {0}"
    )]
    FederationCooldown(#[source] BackoffScheduleError),
    /// A value of `[notifications.delivery]` is one the delivery layer cannot use.
    #[error("invalid configuration: {0}")]
    NotificationDelivery(#[from] DeliveryPolicyError),
    /// `federation.identity.source_id` is an IRI, but not one a stored
    /// resource can be identified by.
    #[error("invalid configuration: federation.identity.source_id: {0}")]
    SourceIdentifier(#[from] IdentifierParseError),
    /// `federation.identity.source_alias` is not a single `received-by` token.
    #[error("invalid configuration: federation.identity.source_alias: {0}")]
    SourceAlias(#[from] ContextSourcePseudonymError),
    /// `federation.identity.extras` holds a value JSON cannot spell, or is no
    /// admissible JSON.
    #[error("invalid configuration: {0}")]
    SourceExtras(#[from] ConfiguredExtrasError),
}

/// Convert every table value whose rules belong to a domain type.
///
/// # Errors
///
/// Returns the [`SectionAdmissionError`] of the first value its domain refuses.
pub fn admit_sections(config: &BrokerConfig) -> Result<(), SectionAdmissionError> {
    check_cors_tokens(&config.http.cors)?;
    intent_retry_schedule(&config.temporal.history).map_err(SectionAdmissionError::TemporalIntentRetry)?;
    check_attribute_nesting(&config.request_limits)?;
    side_effect_retry_schedule(&config.entity.side_effects).map_err(SectionAdmissionError::EntitySideEffectRetry)?;
    side_effect_max_attempts(&config.entity.side_effects)?;
    cooldown_schedule(&config.federation.cooldown).map_err(SectionAdmissionError::FederationCooldown)?;
    check_delivery_policy(&config.notifications.delivery)?;
    let identity = &config.federation.identity;
    Identifier::try_from(&identity.source_id)?;
    ContextSourcePseudonym::try_from(&identity.source_alias)?;
    identity.extras.as_ref().map(JsonLiteral::try_from).transpose()?;
    Ok(())
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
/// columnstore.
#[must_use]
pub(crate) fn expected_deployment(config: &BrokerConfig) -> ExpectedDeployment {
    let columnstore = &config.temporal.storage.columnstore;
    ExpectedDeployment {
        tenant_database_prefix: config.tenancy.databases.name_prefix.clone(), // clone: the controller keeps its own copy of the prefix
        columnstore_conversion: ColumnstoreConversion {
            after: columnstore.convert_after.duration(),
            sweep_interval: columnstore.sweep_interval.duration(),
        },
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
