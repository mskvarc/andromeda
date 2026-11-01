//! The `[notifications]` rules.

use crate::{
    mqtt_packet_limit::MqttPacketLimit,
    schema::notifications::delivery::{MAX_CONCURRENT_DELIVERIES_CEILING, MAX_DELIVERY_ATTEMPTS_CEILING, MAX_DELIVERY_LEASE, MAX_ERROR_LENGTH_CEILING},
    validate::{
        bound::{Quantity, Relation},
        config_key::{ConfigKey, QueueKey, QueueName},
        durable_queue::queue_rules,
        rule::{Limit, Rule},
    },
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
/// The ceilings are what the delivery store and the MQTT protocol hold, and
/// the most slots one delivery pass may keep in flight.
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
            Rule::Ordered {
                key: ConfigKey::Queue(QueueName::NotificationDelivery, QueueKey::LeaseTimeout),
                relation: Relation::AtMost,
                limit: Limit::Fixed(Quantity::Duration(MAX_DELIVERY_LEASE)),
            },
            at_most(
                ConfigKey::NotificationsDeliveryMaxAttempts,
                Quantity::Count(u64::from(MAX_DELIVERY_ATTEMPTS_CEILING)),
            ),
            at_most(
                ConfigKey::NotificationsDeliveryMaxConcurrent,
                Quantity::Count(u64::from(MAX_CONCURRENT_DELIVERIES_CEILING)),
            ),
            at_most(
                ConfigKey::NotificationsDeliveryMaxErrorLength,
                Quantity::Count(u64::from(MAX_ERROR_LENGTH_CEILING)),
            ),
            Rule::Positive(ConfigKey::NotificationsDeliveryMaxMqttPacket),
            at_most(
                ConfigKey::NotificationsDeliveryMaxMqttPacket,
                Quantity::Bytes(u64::from(MqttPacketLimit::MAXIMUM.octets())),
            ),
        ])
}

/// `key` holds at most `ceiling`.
const fn at_most(key: ConfigKey, ceiling: Quantity) -> Rule {
    Rule::Ordered {
        key,
        relation: Relation::AtMost,
        limit: Limit::Fixed(ceiling),
    }
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
    use std::{num::NonZeroU32, time::Duration};

    const LEASE: ConfigKey = ConfigKey::Queue(QueueName::NotificationDelivery, QueueKey::LeaseTimeout);

    /// The refusal of `key` holding `value` past `ceiling`.
    fn past(key: ConfigKey, value: Quantity, ceiling: Quantity) -> ConfigurationViolation {
        ConfigurationViolation::OutOfRange {
            key,
            value,
            relation: Relation::AtMost,
            bound: Bound::Fixed(ceiling),
        }
    }

    #[test]
    fn a_delivery_value_past_its_ceiling_is_refused_and_one_at_it_accepted() {
        let attempts = ConfigKey::NotificationsDeliveryMaxAttempts;
        let mut config = BrokerConfig::default();
        config.notifications.delivery.queue.lease_timeout = HumanDuration::from_secs(86_401);
        config.notifications.delivery.max_attempts = NonZeroU32::MAX;
        config.notifications.delivery.max_concurrent = NonZeroU32::MIN.saturating_add(1_024);
        config.notifications.delivery.max_error_length = NonZeroU32::MIN.saturating_add(2_048);
        assert_eq!(
            refusals(&config),
            [
                past(
                    LEASE,
                    Quantity::Duration(Duration::from_secs(86_401)),
                    Quantity::Duration(Duration::from_hours(24))
                ),
                past(attempts, Quantity::Count(u64::from(u32::MAX)), Quantity::Count(2_147_483_647)),
                past(ConfigKey::NotificationsDeliveryMaxConcurrent, Quantity::Count(1_025), Quantity::Count(1_024)),
                past(ConfigKey::NotificationsDeliveryMaxErrorLength, Quantity::Count(2_049), Quantity::Count(2_048)),
            ]
        );
        config.notifications.delivery.queue.lease_timeout = HumanDuration::from_secs(86_400);
        config.notifications.delivery.max_attempts = NonZeroU32::MIN.saturating_add(2_147_483_646);
        config.notifications.delivery.max_concurrent = NonZeroU32::MIN.saturating_add(1_023);
        config.notifications.delivery.max_error_length = NonZeroU32::MIN.saturating_add(2_047);
        assert_accepted(&config);
    }

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
