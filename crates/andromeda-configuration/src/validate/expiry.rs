//! The `[expiry]` rules.

use crate::{
    schema::expiry::ExpiryConfig,
    validate::{config_key::ConfigKey, violation_log::ViolationLog},
};

/// The sweeps need a positive cadence.
pub(crate) fn check_expiry(expiry: &ExpiryConfig, log: &mut ViolationLog) {
    log.positive_duration(ConfigKey::ExpirySweepInterval, expiry.sweep_interval);
}

#[cfg(test)]
mod tests {
    use crate::validate::{config_key::ConfigKey, rule_tests::assert_positive_duration};

    #[test]
    fn a_zero_sweep_interval_is_refused() {
        assert_positive_duration(|config, value| config.expiry.sweep_interval = value, ConfigKey::ExpirySweepInterval);
    }
}
