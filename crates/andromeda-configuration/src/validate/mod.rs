//! Every rule a loaded configuration passes before the broker reads it.
//!
//! Each table's own bounds live in the module named after the table; the rules
//! that span tables live in `cross_section`. Every rule runs on every pass and
//! records what it finds, so one run reports every rule the configuration
//! breaks. A check that needs a domain type belongs to the crate that owns the
//! type, and the broker composition runs it on a configuration these rules
//! accepted.

pub mod bound;
pub mod config_key;
mod cross_section;
mod database;
mod durable_queue;
mod entity;
mod expiry;
mod federation;
mod http;
mod jsonld;
mod management;
mod notifications;
mod request_limits;
#[cfg(test)]
mod rule_tests;
mod server;
mod snapshot;
mod temporal;
mod tenancy;
pub mod violation;
mod violation_log;

use crate::{
    broker_config::BrokerConfig,
    validate::{
        cross_section::check_cross_section,
        database::check_database,
        entity::check_entity,
        expiry::check_expiry,
        federation::check_federation,
        http::check_http,
        jsonld::check_jsonld,
        management::check_management,
        notifications::check_notifications,
        request_limits::check_request_limits,
        server::check_server,
        snapshot::check_snapshot,
        temporal::check_temporal,
        tenancy::check_tenancy,
        violation::ConfigurationViolation,
        violation_log::ViolationLog,
    },
};
use std::fmt::{self, Display, Formatter};
use thiserror::Error;

/// A loaded configuration that breaks at least one rule, with every rule it
/// breaks.
#[derive(Debug, Error, Eq, PartialEq)]
#[error("invalid configuration:{}", ViolationLines(.violations))]
pub struct InvalidConfiguration {
    /// Every rule broken, in the order the rules ran; never empty.
    violations: Vec<ConfigurationViolation>,
}

impl InvalidConfiguration {
    /// Every rule broken, in the order the rules ran; never empty.
    #[must_use]
    pub fn violations(&self) -> &[ConfigurationViolation] {
        &self.violations
    }

    /// Every rule broken, in the order the rules ran; never empty.
    #[must_use]
    pub fn into_violations(self) -> Vec<ConfigurationViolation> {
        self.violations
    }
}

/// The violations displayed one indented line each.
struct ViolationLines<'violations>(&'violations [ConfigurationViolation]);

// Manual: a derived `Display` formats fixed fields and cannot repeat a line
// for every member of a list.
impl Display for ViolationLines<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|violation| write!(formatter, "\n  {violation}"))
    }
}

/// Run every rule over the configuration.
///
/// # Errors
///
/// Returns [`InvalidConfiguration`] holding every rule the configuration
/// breaks.
pub fn validate(config: &BrokerConfig) -> Result<(), InvalidConfiguration> {
    let mut log = ViolationLog::default();
    check_server(&config.server, &mut log);
    check_management(&config.management, &mut log);
    check_request_limits(&config.request_limits, &mut log);
    check_http(&config.http, &mut log);
    check_database(&config.database, &mut log);
    check_tenancy(&config.tenancy, &mut log);
    check_expiry(&config.expiry, &mut log);
    check_jsonld(&config.jsonld, &mut log);
    check_entity(&config.entity, &mut log);
    check_temporal(&config.temporal, &mut log);
    check_notifications(&config.notifications, &mut log);
    check_federation(&config.federation, &mut log);
    check_snapshot(&config.snapshot, &mut log);
    check_cross_section(config, &mut log);
    log.into_result()
}
