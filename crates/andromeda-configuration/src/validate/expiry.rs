//! The `[expiry]` rules.

use crate::validate::{config_key::ConfigKey, rule::Rule};

/// The sweeps need a positive cadence.
pub(crate) fn rules() -> impl Iterator<Item = Rule> {
    [Rule::Positive(ConfigKey::ExpirySweepInterval)].into_iter()
}

#[cfg(test)]
mod tests {
    use crate::validate::{config_key::ConfigKey, rule_tests::assert_positive_duration};

    #[test]
    fn a_zero_sweep_interval_is_refused() {
        assert_positive_duration(|config, value| config.expiry.sweep_interval = value, ConfigKey::ExpirySweepInterval);
    }
}
