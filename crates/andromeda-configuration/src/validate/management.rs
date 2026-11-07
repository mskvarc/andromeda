//! The `[management]` rules.

use crate::validate::{config_key::ConfigKey, rule::Rule};

/// A separate management port of zero is reserved for dynamic binding and
/// cannot be advertised; in the other modes the port is not read.
pub(crate) fn rules() -> impl Iterator<Item = Rule> {
    [Rule::Positive(ConfigKey::ManagementPort)].into_iter()
}

#[cfg(test)]
mod tests {
    use crate::{
        broker_config::BrokerConfig,
        schema::management::ManagementMode,
        validate::{
            config_key::ConfigKey,
            rule_tests::{assert_accepted, refusals},
            violation::ConfigurationViolation,
        },
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_separate_management_port_of_zero_is_refused_and_port_one_accepted() {
        let mut config = BrokerConfig::local();
        config.management.mode = ManagementMode::SeparatePort;
        config.management.port = 0;
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::Zero {
                key: ConfigKey::ManagementPort
            }]
        );
        config.management.port = 1;
        assert_accepted(&config);
    }

    #[test]
    fn a_management_port_of_zero_is_not_read_outside_separate_port_mode() {
        let mut config = BrokerConfig::local();
        config.management.port = 0;
        for mode in [ManagementMode::SharedPort, ManagementMode::Disabled] {
            config.management.mode = mode;
            assert_accepted(&config);
        }
    }
}
