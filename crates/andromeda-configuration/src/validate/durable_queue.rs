//! The rules every durable queue table follows.

use crate::validate::{
    bound::Relation,
    config_key::{ConfigKey, QueueKey, QueueName},
    rule::{Limit, Rule},
};

/// A worker that polls without pause, an idle backoff ending below where it
/// starts, a claim lapsing as it is taken, a retry with no delay and a retry
/// delay capped below its first step all leave a queue no worker can run.
pub(crate) const fn queue_rules(name: QueueName) -> [Rule; 5] {
    [
        Rule::Positive(ConfigKey::Queue(name, QueueKey::PollInterval)),
        Rule::Ordered {
            key: ConfigKey::Queue(name, QueueKey::MaxPollInterval),
            relation: Relation::AtLeast,
            limit: Limit::Key(ConfigKey::Queue(name, QueueKey::PollInterval)),
        },
        Rule::Positive(ConfigKey::Queue(name, QueueKey::LeaseTimeout)),
        Rule::Positive(ConfigKey::Queue(name, QueueKey::RetryInterval)),
        Rule::Ordered {
            key: ConfigKey::Queue(name, QueueKey::MaxRetryInterval),
            relation: Relation::AtLeast,
            limit: Limit::Key(ConfigKey::Queue(name, QueueKey::RetryInterval)),
        },
    ]
}

#[cfg(test)]
mod tests {
    use crate::{
        broker_config::BrokerConfig,
        human_duration::HumanDuration,
        validate::{
            bound::{Bound, Quantity, Relation},
            config_key::{ConfigKey, QueueKey, QueueName},
            rule_tests::{assert_accepted, assert_positive_duration, refusals},
            violation::ConfigurationViolation,
        },
    };
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    const HISTORY: QueueName = QueueName::TemporalHistory;

    #[test]
    fn a_zero_poll_or_retry_interval_is_refused() {
        assert_positive_duration(
            |config, value| config.temporal.history.queue.poll_interval = value,
            ConfigKey::Queue(HISTORY, QueueKey::PollInterval),
        );
        assert_positive_duration(
            |config, value| config.temporal.history.queue.retry_interval = value,
            ConfigKey::Queue(HISTORY, QueueKey::RetryInterval),
        );
    }

    /// The history queue's lease is bound by nothing else, unlike the
    /// side-effect and delivery leases.
    #[test]
    fn a_zero_lease_is_refused() {
        assert_positive_duration(
            |config, value| config.temporal.history.queue.lease_timeout = value,
            ConfigKey::Queue(HISTORY, QueueKey::LeaseTimeout),
        );
    }

    #[test]
    fn a_poll_ceiling_below_its_floor_is_refused_and_an_equal_one_accepted() {
        let mut config = BrokerConfig::default();
        config.temporal.history.queue.poll_interval = HumanDuration::from_secs(5);
        config.temporal.history.queue.max_poll_interval = HumanDuration::from_millis(4_999);
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::OutOfRange {
                key: ConfigKey::Queue(HISTORY, QueueKey::MaxPollInterval),
                value: Quantity::Duration(Duration::from_millis(4_999)),
                relation: Relation::AtLeast,
                bound: Bound::setting(ConfigKey::Queue(HISTORY, QueueKey::PollInterval), Duration::from_secs(5)),
            }]
        );
        config.temporal.history.queue.max_poll_interval = HumanDuration::from_secs(5);
        assert_accepted(&config);
    }

    #[test]
    fn a_retry_ceiling_below_its_first_step_is_refused_and_an_equal_one_accepted() {
        let mut config = BrokerConfig::default();
        config.entity.side_effects.queue.retry_interval = HumanDuration::from_secs(10);
        config.entity.side_effects.queue.max_retry_interval = HumanDuration::from_millis(9_999);
        let side_effects = QueueName::EntitySideEffects;
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::OutOfRange {
                key: ConfigKey::Queue(side_effects, QueueKey::MaxRetryInterval),
                value: Quantity::Duration(Duration::from_millis(9_999)),
                relation: Relation::AtLeast,
                bound: Bound::setting(ConfigKey::Queue(side_effects, QueueKey::RetryInterval), Duration::from_secs(10)),
            }]
        );
        config.entity.side_effects.queue.max_retry_interval = HumanDuration::from_secs(10);
        assert_accepted(&config);
    }

    #[test]
    fn a_queue_violation_names_the_queue_key_path() {
        let mut config = BrokerConfig::default();
        config.notifications.delivery.queue.retry_interval = HumanDuration::from_secs(0);
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::Zero {
                key: ConfigKey::Queue(QueueName::NotificationDelivery, QueueKey::RetryInterval)
            }]
        );
    }
}
