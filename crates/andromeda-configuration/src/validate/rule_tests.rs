//! What the rule tests compare: the violations a whole configuration is
//! refused for, starting from the defaults.

use crate::{
    broker_config::BrokerConfig,
    human_duration::HumanDuration,
    validate::{InvalidConfiguration, config_key::ConfigKey, validate, violation::ConfigurationViolation},
};
use pretty_assertions::assert_eq;

/// Every violation `config` is refused for, empty when it is valid.
pub(crate) fn refusals(config: &BrokerConfig) -> Vec<ConfigurationViolation> {
    validate(config).map_or_else(InvalidConfiguration::into_violations, |()| Vec::new())
}

/// Assert that `config` breaks no rule.
pub(crate) fn assert_accepted(config: &BrokerConfig) {
    let refused = refusals(config);
    assert!(refused.is_empty(), "{refused:?}");
}

/// Assert that the duration `set` writes is refused at zero, alone, and
/// accepted one millisecond above it.
pub(crate) fn assert_positive_duration(set: fn(&mut BrokerConfig, HumanDuration), key: ConfigKey) {
    let mut config = BrokerConfig::default();
    set(&mut config, HumanDuration::from_millis(0));
    assert_eq!(refusals(&config), [ConfigurationViolation::Zero { key }], "{key}");
    set(&mut config, HumanDuration::from_millis(1));
    assert_accepted(&config);
}

#[test]
fn the_default_configuration_breaks_no_rule() {
    assert_accepted(&BrokerConfig::default());
}

/// Validation goes on past a broken rule, so a file breaking three rules in
/// three tables is refused for all three, in the order the rules run.
#[test]
fn a_configuration_breaking_three_rules_is_refused_for_all_three() {
    let mut config = BrokerConfig::default();
    config.server.port = 0;
    config.expiry.sweep_interval = HumanDuration::from_secs(0);
    config.snapshot.preparation.lease_timeout = HumanDuration::from_secs(0);
    assert_eq!(
        refusals(&config),
        [
            ConfigurationViolation::Zero { key: ConfigKey::ServerPort },
            ConfigurationViolation::Zero {
                key: ConfigKey::ExpirySweepInterval
            },
            ConfigurationViolation::Zero {
                key: ConfigKey::SnapshotPreparationLeaseTimeout
            },
        ]
    );
}

#[test]
fn the_refusal_lists_every_violation_on_its_own_line() {
    let mut config = BrokerConfig::default();
    config.server.port = 0;
    config.expiry.sweep_interval = HumanDuration::from_secs(0);
    let message = validate(&config).map_err(|error| error.to_string());
    assert_eq!(
        message,
        Err("invalid configuration:\n  server.port must be greater than zero\n  expiry.sweep_interval must be greater than zero".to_owned())
    );
}
