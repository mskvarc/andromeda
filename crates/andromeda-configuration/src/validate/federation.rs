//! The `[federation]` rules. The cooldown schedule is checked by the
//! forwarding layer that builds it.

use crate::{
    schema::federation::FederationConfig,
    validate::{config_key::ConfigKey, violation_log::ViolationLog},
};

/// Connecting to a source and a forwarded request each need a positive
/// deadline.
pub(crate) fn check_federation(federation: &FederationConfig, log: &mut ViolationLog) {
    log.positive_duration(ConfigKey::FederationForwardingConnectTimeout, federation.forwarding.connect_timeout);
    log.positive_duration(ConfigKey::FederationForwardingRequestTimeout, federation.forwarding.request_timeout);
}

#[cfg(test)]
mod tests {
    use crate::validate::{config_key::ConfigKey, rule_tests::assert_positive_duration};

    #[test]
    fn a_zero_connect_or_request_timeout_is_refused() {
        assert_positive_duration(
            |config, value| config.federation.forwarding.connect_timeout = value,
            ConfigKey::FederationForwardingConnectTimeout,
        );
        assert_positive_duration(
            |config, value| config.federation.forwarding.request_timeout = value,
            ConfigKey::FederationForwardingRequestTimeout,
        );
    }
}
