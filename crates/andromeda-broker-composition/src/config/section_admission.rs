//! Admitting each loaded section into the domain values it is read as.
//!
//! A section's own values are checked where the section is defined; a value
//! whose rules belong to a domain type is checked by converting it into that
//! type, which only the composition, seeing every domain crate, can do. Every
//! conversion runs once while the configuration loads, so a broker never
//! starts with a value a domain would refuse later. The derivations that read
//! more than one section live here too.

use andromeda_configuration::{
    broker_config::BrokerConfig,
    schema::{extensions::ExtensionsConfig, feature_mode::HttpFeatureMode, pagination::PaginationConfig},
};
use andromeda_entity::configured_policy::{AttributeNestingAboveDecodable, check_attribute_nesting, pending_side_effects_retry_schedule};
use andromeda_federation::cooldown_policy::cooldown_schedule;
use andromeda_http_stack::cors_tokens::{CorsTokenError, check_cors_tokens};
use andromeda_json_literal::{JsonLiteral, configured_extras::ConfiguredExtrasError};
use andromeda_ngsi_ld_lexicon::{
    context_source_pseudonym::{ContextSourcePseudonym, ContextSourcePseudonymError},
    identifier::{Identifier, IdentifierParseError},
};
use andromeda_ngsi_ld_request::pagination::{
    cursor_availability::CursorAvailability,
    policy::{PaginationPolicy, PaginationPolicyError},
};
use andromeda_notification::delivery_policy::{DeliveryPolicyError, check_delivery_policy};
use andromeda_postgresql_context_store::database_defaults::{DatabaseDefaults, StatementTimeout};
use andromeda_postgresql_maintenance::expected_deployment::{ColumnstoreConversion, ExpectedDeployment};
use andromeda_retry::schedule_error::BackoffScheduleError;
use andromeda_temporal::configured_policy::intent_retry_schedule;
use thiserror::Error;

/// A loaded section holds a value its domain refuses.
#[derive(Debug, Error)]
pub enum SectionAdmissionError {
    /// A CORS method or header name is not one the HTTP layer reads.
    #[error("invalid configuration: {0}")]
    HttpCors(#[from] CorsTokenError),
    /// `temporal.intent_retry` and `temporal.intent_retry_max` do not bound a
    /// retry schedule: the delay is zero or the ceiling is below it.
    #[error("invalid configuration: temporal.intent_retry and temporal.intent_retry_max do not form a retry schedule: {0}")]
    TemporalIntentRetry(#[source] BackoffScheduleError),
    /// `entity.max_attribute_nesting` passes the depth every stored value reads back at.
    #[error("invalid configuration: {0}")]
    EntityAttributeNesting(#[from] AttributeNestingAboveDecodable),
    /// `entity.pending_side_effects_base_backoff` does not bound a retry schedule.
    #[error("invalid configuration: entity.pending_side_effects_base_backoff does not form a retry schedule: {0}")]
    EntitySideEffectRetry(#[source] BackoffScheduleError),
    /// A cooldown that holds sources back needs a positive window, and an
    /// exponential one a cap no shorter than its first window.
    #[error(
        "invalid configuration: federation.cooldown_window and federation.cooldown_max do not form a cooldown schedule unless federation.cooldown_mode is off: {0}"
    )]
    FederationCooldown(#[source] BackoffScheduleError),
    /// A delivery value in `[notifications.delivery]` is one the delivery layer cannot use.
    #[error("invalid configuration: {0}")]
    NotificationDelivery(#[from] DeliveryPolicyError),
    /// `identity.source_id` is an IRI, but not one a stored resource can be
    /// identified by.
    #[error("invalid configuration: identity.source_id: {0}")]
    SourceIdentifier(#[from] IdentifierParseError),
    /// `identity.source_alias` is not a single `received-by` token.
    #[error("invalid configuration: identity.source_alias: {0}")]
    SourceAlias(#[from] ContextSourcePseudonymError),
    /// `identity.source_extras` holds a value JSON cannot spell, or is no
    /// admissible JSON.
    #[error("invalid configuration: {0}")]
    SourceExtras(#[from] ConfiguredExtrasError),
    /// `[pagination]` and `[extensions]` together name no pagination policy.
    #[error("invalid configuration: {0}")]
    Pagination(#[from] PaginationPolicyError),
}

/// Convert every section value whose rules belong to a domain type.
///
/// # Errors
///
/// Returns the [`SectionAdmissionError`] of the first value its domain refuses.
pub fn admit_sections(config: &BrokerConfig) -> Result<(), SectionAdmissionError> {
    check_cors_tokens(&config.http.cors)?;
    intent_retry_schedule(&config.temporal).map_err(SectionAdmissionError::TemporalIntentRetry)?;
    check_attribute_nesting(&config.entity)?;
    pending_side_effects_retry_schedule(&config.entity).map_err(SectionAdmissionError::EntitySideEffectRetry)?;
    cooldown_schedule(&config.federation).map_err(SectionAdmissionError::FederationCooldown)?;
    check_delivery_policy(&config.notifications.delivery)?;
    Identifier::try_from(&config.identity.source_id)?;
    ContextSourcePseudonym::try_from(&config.identity.source_alias)?;
    config.identity.source_extras.as_ref().map(JsonLiteral::try_from).transpose()?;
    resolve_pagination(&config.extensions, config.pagination)?;
    Ok(())
}

/// Whether cursor pagination is available, and who turned it off if not.
///
/// The conformance override forces every extension off whatever its field
/// says, so it is consulted first.
pub(crate) const fn cursor_availability(extensions: &ExtensionsConfig) -> CursorAvailability {
    match (extensions.conformance_mode, extensions.cursor_pagination) {
        (HttpFeatureMode::Enabled, HttpFeatureMode::Enabled | HttpFeatureMode::Disabled) => CursorAvailability::ForcedOffByConformance,
        (HttpFeatureMode::Disabled, HttpFeatureMode::Disabled) => CursorAvailability::DisabledByOperator,
        (HttpFeatureMode::Disabled, HttpFeatureMode::Enabled) => CursorAvailability::Enabled,
    }
}

/// The pagination policy `extensions` and `pagination` resolve to.
///
/// # Errors
///
/// Returns [`PaginationPolicyError::CursorDefaultWithoutCursorPagination`] when
/// the operator disabled cursor pagination and left cursors the default mode.
pub(crate) const fn resolve_pagination(extensions: &ExtensionsConfig, pagination: PaginationConfig) -> Result<PaginationPolicy, PaginationPolicyError> {
    PaginationPolicy::resolve(pagination.default_mode, cursor_availability(extensions))
}

/// The session defaults installed on every database the broker serves from:
/// the configured commit durability, and a statement timeout just past the
/// HTTP request deadline.
#[must_use]
pub(crate) const fn database_defaults(config: &BrokerConfig) -> DatabaseDefaults {
    DatabaseDefaults {
        synchronous_commit: config.database.synchronous_commit,
        statement_timeout: StatementTimeout::past_request_deadline(config.http.request_timeout.duration()),
    }
}

/// What the maintenance controller checks the cluster against: the prefix of
/// every tenant database name, and how temporal history is converted to the
/// columnstore.
#[must_use]
pub(crate) fn expected_deployment(config: &BrokerConfig) -> ExpectedDeployment {
    ExpectedDeployment {
        tenant_database_prefix: config.tenancy.tenant_db_prefix.clone(), // clone: the controller keeps its own copy of the prefix
        columnstore_conversion: ColumnstoreConversion {
            after: config.temporal.columnstore_after.duration(),
            sweep_interval: config.temporal.columnstore_sweep_interval.duration(),
        },
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
