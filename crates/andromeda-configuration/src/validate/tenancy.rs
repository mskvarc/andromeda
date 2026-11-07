//! The `[tenancy]` rules.

use crate::validate::{
    bound::Relation,
    config_key::ConfigKey,
    rule::{Limit, Rule},
};

/// Every tenancy duration is positive. A runtime woken for due work lives
/// shorter than an idle one, so it is the first eviction candidate yet never
/// displaces a runtime a request touched.
pub(crate) fn rules() -> impl Iterator<Item = Rule> {
    [
        Rule::Positive(ConfigKey::TenancyProvisionalTenantLifetime),
        Rule::Positive(ConfigKey::TenancyResidencyIdleEvictionAfter),
        Rule::Positive(ConfigKey::TenancyResidencySweepInterval),
        Rule::Positive(ConfigKey::TenancyResidencyMinWakeInterval),
        Rule::Positive(ConfigKey::TenancyResidencyWokenLifetime),
        Rule::Ordered {
            key: ConfigKey::TenancyResidencyWokenLifetime,
            relation: Relation::Below,
            limit: Limit::Key(ConfigKey::TenancyResidencyIdleEvictionAfter),
        },
        Rule::Positive(ConfigKey::TenancyBuildAdmissionTimeout),
    ]
    .into_iter()
}

#[cfg(test)]
mod tests {
    use crate::{
        broker_config::BrokerConfig,
        human_duration::HumanDuration,
        validate::{
            bound::{Bound, Quantity, Relation},
            config_key::ConfigKey,
            rule_tests::{assert_accepted, assert_positive_duration, refusals},
            violation::ConfigurationViolation,
        },
    };
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn a_zero_residency_sweep_wake_or_admission_duration_is_refused() {
        assert_positive_duration(
            |config, value| config.tenancy.residency.sweep_interval = value,
            ConfigKey::TenancyResidencySweepInterval,
        );
        assert_positive_duration(
            |config, value| config.tenancy.residency.min_wake_interval = value,
            ConfigKey::TenancyResidencyMinWakeInterval,
        );
        assert_positive_duration(
            |config, value| config.tenancy.residency.woken_lifetime = value,
            ConfigKey::TenancyResidencyWokenLifetime,
        );
        assert_positive_duration(
            |config, value| config.tenancy.build.admission_timeout = value,
            ConfigKey::TenancyBuildAdmissionTimeout,
        );
    }

    /// A zero lifetime is also not above the request timeout it must exceed.
    #[test]
    fn a_zero_provisional_tenant_lifetime_is_refused_with_the_request_timeout_it_must_exceed() {
        let mut config = BrokerConfig::local();
        config.tenancy.provisional_tenant_lifetime = HumanDuration::from_secs(0);
        assert_eq!(
            refusals(&config),
            [
                ConfigurationViolation::Zero {
                    key: ConfigKey::TenancyProvisionalTenantLifetime
                },
                ConfigurationViolation::OutOfRange {
                    key: ConfigKey::TenancyProvisionalTenantLifetime,
                    value: Quantity::Duration(Duration::ZERO),
                    relation: Relation::Above,
                    bound: Bound::setting(ConfigKey::RequestLimitsTimeout, Duration::from_secs(30)),
                },
            ]
        );
    }

    /// A zero idle eviction is also no longer than the woken lifetime that
    /// must stay below it.
    #[test]
    fn a_zero_idle_eviction_is_refused_with_the_woken_lifetime_it_must_exceed() {
        let mut config = BrokerConfig::local();
        config.tenancy.residency.idle_eviction_after = HumanDuration::from_secs(0);
        assert_eq!(
            refusals(&config),
            [
                ConfigurationViolation::Zero {
                    key: ConfigKey::TenancyResidencyIdleEvictionAfter
                },
                ConfigurationViolation::OutOfRange {
                    key: ConfigKey::TenancyResidencyWokenLifetime,
                    value: Quantity::Duration(Duration::from_secs(60)),
                    relation: Relation::Below,
                    bound: Bound::setting(ConfigKey::TenancyResidencyIdleEvictionAfter, Duration::ZERO),
                },
            ]
        );
    }

    #[test]
    fn a_woken_lifetime_at_the_idle_eviction_is_refused_and_one_step_below_accepted() {
        let mut config = BrokerConfig::local();
        config.tenancy.residency.idle_eviction_after = HumanDuration::from_secs(60);
        config.tenancy.residency.woken_lifetime = HumanDuration::from_secs(60);
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::OutOfRange {
                key: ConfigKey::TenancyResidencyWokenLifetime,
                value: Quantity::Duration(Duration::from_secs(60)),
                relation: Relation::Below,
                bound: Bound::setting(ConfigKey::TenancyResidencyIdleEvictionAfter, Duration::from_secs(60)),
            }]
        );
        config.tenancy.residency.woken_lifetime = HumanDuration::from_millis(59_999);
        assert_accepted(&config);
    }
}
