//! The `[notifications]` rules.

use crate::validate::{
    bound::Relation,
    config_key::{ConfigKey, QueueKey, QueueName},
    durable_queue::queue_rules,
    rule::{Limit, Rule},
};
use std::time::Duration;

/// How much longer than `request_timeout` a delivery lease must run.
///
/// One lease covers one delivery attempt from its claim to its
/// acknowledgement. The POST is bounded by `request_timeout`, but the attempt
/// also re-reads its subscription and the entity it notifies before sending,
/// and writes its acknowledgement after: three store round trips the timeout
/// does not bound. This margin is what they may take together before a second
/// worker could reclaim the delivery and send it again.
const DELIVERY_LEASE_MARGIN: Duration = Duration::from_secs(5);

/// An outbound notification needs a positive deadline, the queue the queue
/// rules, its lease the margin over one attempt, and every retention a
/// positive age and cadence, as the periodic drain a positive sleep bound.
pub(crate) fn rules() -> impl Iterator<Item = Rule> {
    [Rule::Positive(ConfigKey::NotificationsDeliveryRequestTimeout)]
        .into_iter()
        .chain(queue_rules(QueueName::NotificationDelivery))
        .chain([
            Rule::Ordered {
                key: ConfigKey::Queue(QueueName::NotificationDelivery, QueueKey::LeaseTimeout),
                relation: Relation::AtLeast,
                limit: Limit::KeyPlusMargin(ConfigKey::NotificationsDeliveryRequestTimeout, DELIVERY_LEASE_MARGIN),
            },
            Rule::Positive(ConfigKey::NotificationsDeliveryAcknowledgedAfter),
            Rule::Positive(ConfigKey::NotificationsDeliveryDeadLetterAfter),
            Rule::Positive(ConfigKey::NotificationsDeliveryOrphanEventAfter),
            Rule::Positive(ConfigKey::NotificationsDeliveryCleanupInterval),
            Rule::Positive(ConfigKey::NotificationsPeriodicMaxPollInterval),
        ])
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

    const LEASE: ConfigKey = ConfigKey::Queue(QueueName::NotificationDelivery, QueueKey::LeaseTimeout);

    #[test]
    fn a_zero_request_timeout_retention_or_periodic_sleep_is_refused() {
        assert_positive_duration(
            |config, value| config.notifications.delivery.request_timeout = value,
            ConfigKey::NotificationsDeliveryRequestTimeout,
        );
        assert_positive_duration(
            |config, value| config.notifications.delivery.retention.acknowledged_after = value,
            ConfigKey::NotificationsDeliveryAcknowledgedAfter,
        );
        assert_positive_duration(
            |config, value| config.notifications.delivery.retention.dead_letter_after = value,
            ConfigKey::NotificationsDeliveryDeadLetterAfter,
        );
        assert_positive_duration(
            |config, value| config.notifications.delivery.retention.orphan_event_after = value,
            ConfigKey::NotificationsDeliveryOrphanEventAfter,
        );
        assert_positive_duration(
            |config, value| config.notifications.delivery.retention.cleanup_interval = value,
            ConfigKey::NotificationsDeliveryCleanupInterval,
        );
        assert_positive_duration(
            |config, value| config.notifications.periodic.max_poll_interval = value,
            ConfigKey::NotificationsPeriodicMaxPollInterval,
        );
    }

    #[test]
    fn a_delivery_lease_short_of_its_margin_is_refused_and_the_margin_exactly_accepted() {
        let mut config = BrokerConfig::default();
        config.notifications.delivery.request_timeout = HumanDuration::from_secs(10);
        config.notifications.delivery.queue.lease_timeout = HumanDuration::from_millis(14_999);
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::OutOfRange {
                key: LEASE,
                value: Quantity::Duration(Duration::from_millis(14_999)),
                relation: Relation::AtLeast,
                bound: Bound::SettingPlusMargin {
                    key: ConfigKey::NotificationsDeliveryRequestTimeout,
                    value: Duration::from_secs(10),
                    margin: Duration::from_secs(5),
                },
            }]
        );
        config.notifications.delivery.queue.lease_timeout = HumanDuration::from_secs(15);
        assert_accepted(&config);
    }

    /// A zero lease is also short of the margin over the request timeout.
    #[test]
    fn a_zero_delivery_lease_is_refused_with_the_margin_it_lacks() {
        let mut config = BrokerConfig::default();
        config.notifications.delivery.queue.lease_timeout = HumanDuration::from_secs(0);
        assert_eq!(
            refusals(&config),
            [
                ConfigurationViolation::Zero { key: LEASE },
                ConfigurationViolation::OutOfRange {
                    key: LEASE,
                    value: Quantity::Duration(Duration::ZERO),
                    relation: Relation::AtLeast,
                    bound: Bound::SettingPlusMargin {
                        key: ConfigKey::NotificationsDeliveryRequestTimeout,
                        value: Duration::from_secs(10),
                        margin: Duration::from_secs(5),
                    },
                },
            ]
        );
    }
}
