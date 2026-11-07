//! Every rule a loaded configuration passes before the broker reads it.
//!
//! Each table's own rules live in the module named after the table; the rules
//! that span tables live in `cross_section`. Every rule is declared as data,
//! so the configuration descriptor lists the rules the validator runs: a
//! comparison of one key with zero, a fixed limit or another key is a
//! [`Rule`], and any other rule is a [`CompoundRule`], conditions under which
//! it applies and obligations it then asks. Every rule runs on every pass and
//! records what it finds, so one run reports every rule the configuration
//! breaks. A bound a domain type also holds is declared here
//! too, from a constant this crate owns, so the descriptor states it; a check
//! that needs a domain type to decide belongs to the crate that owns the type,
//! and the broker composition runs it on a configuration these rules accepted.

mod address_width;
pub mod bound;
pub mod choice_value;
mod compound_check;
pub mod compound_rule;
pub mod config_key;
mod cross_section;
mod database;
mod durable_queue;
mod entity;
mod expiry;
mod federation;
mod http;
mod jsonld;
mod key_holding;
mod key_reading;
mod management;
mod notifications;
mod request_limits;
pub mod rule;
#[cfg(test)]
mod rule_tests;
mod server;
mod snapshot;
mod temporal;
mod tenancy;
mod tls;
pub mod violation;
mod violation_log;

use crate::{
    broker_config::BrokerConfig,
    validate::{address_width::AddressWidth, compound_rule::CompoundRule, rule::Rule, violation::ConfigurationViolation, violation_log::ViolationLog},
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

/// Every comparison rule a configuration passes, table by table, the rules
/// spanning tables last.
pub fn declared_rules() -> impl Iterator<Item = Rule> {
    server::rules()
        .chain(tls::rules())
        .chain(management::rules())
        .chain(request_limits::rules())
        .chain(database::rules())
        .chain(tenancy::rules())
        .chain(expiry::rules())
        .chain(jsonld::rules())
        .chain(entity::rules())
        .chain(temporal::rules())
        .chain(notifications::rules())
        .chain(federation::rules())
        .chain(snapshot::rules())
        .chain(cross_section::rules())
}

/// Every rule that is no single comparison, table by table, the rules
/// spanning tables last.
pub fn compound_rules() -> impl Iterator<Item = CompoundRule> {
    request_limits::COMPOUND_RULES
        .into_iter()
        .chain(http::COMPOUND_RULES)
        .chain(jsonld::COMPOUND_RULES)
        .chain(temporal::COMPOUND_RULES)
        .chain(federation::COMPOUND_RULES)
        .chain(cross_section::COMPOUND_RULES)
}

/// Run every rule over the configuration: the comparisons first, then the
/// rules that are none.
///
/// # Errors
///
/// Returns [`InvalidConfiguration`] holding every rule the configuration
/// breaks.
pub fn validate(config: &BrokerConfig) -> Result<(), InvalidConfiguration> {
    validate_for(config, AddressWidth::PLATFORM)
}

/// [`validate`] with byte sizes checked against `width` in place of the
/// platform's address width.
pub(crate) fn validate_for(config: &BrokerConfig, width: AddressWidth) -> Result<(), InvalidConfiguration> {
    let mut log = ViolationLog::default();
    declared_rules().for_each(|rule| log.apply(rule, config));
    compound_rules().for_each(|rule| log.check(&rule, config, width));
    log.into_result()
}
