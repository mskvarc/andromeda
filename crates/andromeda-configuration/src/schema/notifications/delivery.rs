//! The `[notifications.delivery]` tables: how notifications reach their
//! subscribers, the durable queue that carries them, and how long a finished
//! delivery is kept.

use crate::{
    human_duration::HumanDuration,
    schema::durable_queue::{DurableQueue, DurableQueueError, QueueName},
};
use serde::{Deserialize, Serialize};
use std::{num::NonZeroU32, time::Duration};
use strum::{Display, EnumString};
use thiserror::Error;
use toml_comment::TomlComment;

/// How much longer than `request_timeout` a delivery lease must run.
///
/// One lease covers one delivery attempt from its claim to its
/// acknowledgement. The POST is bounded by `request_timeout`, but the attempt
/// also re-reads its subscription and the entity it notifies before sending,
/// and writes its acknowledgement after: three store round trips the timeout
/// does not bound. This margin is what they may take together before a second
/// worker could reclaim the delivery and send it again.
pub const DELIVERY_LEASE_MARGIN: Duration = Duration::from_secs(5);

/// Invalid notification delivery configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum DeliveryConfigError {
    /// The outbound notification request timeout was configured as zero.
    #[error("notifications.delivery.request_timeout must be greater than zero")]
    ZeroRequestTimeout,
    /// The queue table is unusable.
    #[error(transparent)]
    Queue(#[from] DurableQueueError),
    /// The claim lease did not exceed the outbound request timeout by
    /// [`DELIVERY_LEASE_MARGIN`], so a delivery still in flight could be
    /// claimed and sent again by a second worker.
    #[error(
        "notifications.delivery.queue.lease_timeout must exceed notifications.delivery.request_timeout by at least {margin_seconds}s, the store round trips around one delivery attempt"
    )]
    LeaseWithoutMarginOverRequestTimeout {
        /// The margin the lease must leave over the request timeout, in seconds.
        margin_seconds: u64,
    },
    /// A retention age or the cleanup cadence was configured as zero.
    #[error("notifications.delivery.retention.{0} must be greater than zero")]
    ZeroRetention(DeliveryRetentionKey),
}

/// A key of `[notifications.delivery.retention]`, for naming the one refused.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum DeliveryRetentionKey {
    /// `acknowledged_after`.
    AcknowledgedAfter,
    /// `dead_letter_after`.
    DeadLetterAfter,
    /// `orphan_event_after`.
    OrphanEventAfter,
    /// `cleanup_interval`.
    CleanupInterval,
}

/// How notifications are delivered to HTTP and MQTT subscribers.
///
/// Both deliver every notification; they differ only in durability.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum DeliveryMode {
    /// Subscriptions are evaluated and notifications delivered in-process,
    /// without persistence, retry, or dead-lettering. A crashed broker loses
    /// only the notifications still in flight. Deliveries a durable broker
    /// already queued in the tenant database are still drained, with their
    /// retry.
    #[default]
    Live,
    /// Every notification is recorded in the durable queue in the commit that
    /// raised it, and delivered with retry, leasing and dead-lettering.
    Durable,
}

/// Whether a notification may be sent to an endpoint that resolves into
/// private or reserved address space.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum PrivateAddresses {
    /// Any endpoint the broker's network reaches is notified.
    #[default]
    Allowed,
    /// An HTTP endpoint whose resolved address is loopback, private or
    /// reserved is refused, so a client-supplied subscription cannot aim the
    /// broker at internal infrastructure.
    Refused,
}

/// How long a finished delivery is kept.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeliveryRetentionConfig {
    /// How long an acknowledged delivery is kept, in human-readable form
    /// (`"1day"`).
    #[toml_comment(inline)]
    pub acknowledged_after: HumanDuration,
    /// How long a delivery that ran out of attempts is kept, in
    /// human-readable form (`"7days"`): longer than an acknowledged one, as
    /// evidence of a notification that never arrived.
    #[toml_comment(inline)]
    pub dead_letter_after: HumanDuration,
    /// How long an event no delivery refers to is kept, in human-readable
    /// form (`"1day"`).
    #[toml_comment(inline)]
    pub orphan_event_after: HumanDuration,
    /// How often expired deliveries and events are removed, in human-readable
    /// form (`"5m"`). A pass runs only when the tenant may have written since
    /// the last one.
    #[toml_comment(inline)]
    pub cleanup_interval: HumanDuration,
}

impl Default for DeliveryRetentionConfig {
    fn default() -> DeliveryRetentionConfig {
        DeliveryRetentionConfig {
            acknowledged_after: HumanDuration::from_days(1),
            dead_letter_after: HumanDuration::from_days(7),
            orphan_event_after: HumanDuration::from_days(1),
            cleanup_interval: HumanDuration::from_secs(300),
        }
    }
}

/// How notifications reach their subscribers.
///
/// `mode` decides only how a commit's notifications reach their subscribers.
/// The queue and its retention govern the delivery drain, which runs under
/// either mode: the queue lives in the tenant database, and a durable broker
/// sharing it, or this broker before it was switched to live delivery, may
/// have queued deliveries that are still due. `request_timeout` bounds every
/// outbound notification, queued or live.
#[derive(Clone, Copy, Deserialize, Serialize, TomlComment, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeliveryConfig {
    /// Whether notifications are delivered `live` (in-process) or `durable`
    /// (queued).
    #[toml_comment(inline)]
    pub mode: DeliveryMode,
    /// How long an outbound notification may take before it is abandoned, in
    /// human-readable form (`"10s"`). A subscription's own Table 5.2.15-1
    /// `timeout` may only shorten it.
    #[toml_comment(inline)]
    pub request_timeout: HumanDuration,
    /// The most notifications one delivery pass sends at once.
    #[toml_comment(inline)]
    pub max_concurrent: NonZeroU32,
    /// The most characters of a failure diagnostic kept when a failed attempt
    /// is recorded; at most the longest failure summary the store keeps.
    #[toml_comment(inline)]
    pub max_error_length: NonZeroU32,
    /// Whether an endpoint resolving into private or reserved address space is
    /// `allowed` or `refused`. Checked against the address actually resolved.
    #[toml_comment(inline)]
    pub private_addresses: PrivateAddresses,
    /// How the queue is polled, claimed and retried. A delivery is retried on
    /// a delay that doubles from `retry_interval`, and dead-lettered after
    /// `max_attempts`. `lease_timeout` must exceed `request_timeout` by five
    /// seconds.
    pub queue: DurableQueue,
    /// How long a finished delivery is kept.
    pub retention: DeliveryRetentionConfig,
}

impl Default for DeliveryConfig {
    fn default() -> DeliveryConfig {
        DeliveryConfig {
            mode: DeliveryMode::Live,
            request_timeout: HumanDuration::from_secs(10),
            max_concurrent: NonZeroU32::MIN.saturating_add(19),
            max_error_length: NonZeroU32::MIN.saturating_add(1023),
            private_addresses: PrivateAddresses::Allowed,
            queue: DurableQueue {
                batch_size: NonZeroU32::MIN.saturating_add(49),
                retry_interval: HumanDuration::from_secs(5),
                ..DurableQueue::default()
            },
            retention: DeliveryRetentionConfig::default(),
        }
    }
}

impl DeliveryConfig {
    /// Validate the timeout, the queue, the lease margin and the retention.
    ///
    /// The attempt ceiling, diagnostic bound and in-flight bound are the
    /// delivery layer's own values and are checked there.
    ///
    /// # Errors
    ///
    /// Returns the [`DeliveryConfigError`] naming the first value refused.
    pub const fn validate(&self) -> Result<(), DeliveryConfigError> {
        if self.request_timeout.is_zero() {
            return Err(DeliveryConfigError::ZeroRequestTimeout);
        }
        if let Err(error) = self.queue.validate(QueueName::NotificationDelivery) {
            return Err(DeliveryConfigError::Queue(error));
        }
        if self.queue.lease_timeout.duration().as_nanos() < self.request_timeout.duration().saturating_add(DELIVERY_LEASE_MARGIN).as_nanos() {
            return Err(DeliveryConfigError::LeaseWithoutMarginOverRequestTimeout {
                margin_seconds: DELIVERY_LEASE_MARGIN.as_secs(),
            });
        }
        let retention = &self.retention;
        if retention.acknowledged_after.is_zero() {
            Err(DeliveryConfigError::ZeroRetention(DeliveryRetentionKey::AcknowledgedAfter))
        } else if retention.dead_letter_after.is_zero() {
            Err(DeliveryConfigError::ZeroRetention(DeliveryRetentionKey::DeadLetterAfter))
        } else if retention.orphan_event_after.is_zero() {
            Err(DeliveryConfigError::ZeroRetention(DeliveryRetentionKey::OrphanEventAfter))
        } else if retention.cleanup_interval.is_zero() {
            Err(DeliveryConfigError::ZeroRetention(DeliveryRetentionKey::CleanupInterval))
        } else {
            Ok(())
        }
    }

    /// Whether notifications are recorded and delivered through the durable queue.
    #[must_use]
    pub const fn is_durable(&self) -> bool {
        matches!(self.mode, DeliveryMode::Durable)
    }

    /// The outbound notification request timeout.
    #[must_use]
    pub const fn request_timeout(&self) -> Duration {
        self.request_timeout.duration()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        human_duration::HumanDuration,
        schema::notifications::delivery::{DeliveryConfig, DeliveryConfigError, DeliveryMode, DeliveryRetentionKey, PrivateAddresses},
    };
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    #[test]
    fn the_defaults_deliver_live_to_any_address_and_are_valid() {
        let config = DeliveryConfig::default();
        assert_eq!(config.mode, DeliveryMode::Live);
        assert_eq!(config.private_addresses, PrivateAddresses::Allowed);
        assert_eq!(config.queue.batch_size.get(), 50);
        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn private_addresses_read_their_two_values_and_refuse_any_other() {
        assert_eq!(PrivateAddresses::from_str("refused"), Ok(PrivateAddresses::Refused));
        assert_eq!(PrivateAddresses::from_str("allowed"), Ok(PrivateAddresses::Allowed));
        assert!(PrivateAddresses::from_str("enabled").is_err());
    }

    #[test]
    fn a_lease_within_the_margin_of_the_request_timeout_is_refused() {
        let mut config = DeliveryConfig::default();
        config.queue.lease_timeout = HumanDuration::from_millis(14_999);
        assert_eq!(
            config.validate(),
            Err(DeliveryConfigError::LeaseWithoutMarginOverRequestTimeout { margin_seconds: 5 })
        );
        config.queue.lease_timeout = HumanDuration::from_secs(15);
        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn a_zero_retention_value_is_refused_by_its_key() {
        let mut config = DeliveryConfig::default();
        config.retention.orphan_event_after = HumanDuration::from_secs(0);
        let refused = config.validate();
        assert_eq!(refused, Err(DeliveryConfigError::ZeroRetention(DeliveryRetentionKey::OrphanEventAfter)));
        assert_eq!(
            refused.map_err(|error| error.to_string()),
            Err("notifications.delivery.retention.orphan_event_after must be greater than zero".to_owned())
        );
    }

    #[test]
    fn a_zero_request_timeout_is_refused() {
        let config = DeliveryConfig {
            request_timeout: HumanDuration::from_secs(0),
            ..DeliveryConfig::default()
        };
        assert_eq!(config.validate(), Err(DeliveryConfigError::ZeroRequestTimeout));
    }
}
