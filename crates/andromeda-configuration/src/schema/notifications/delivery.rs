//! The `[notifications.delivery]` tables: how notifications reach their
//! subscribers, the durable queue that carries them, and how long a finished
//! delivery is kept.

use crate::{
    address_rule::AddressRule,
    human_duration::HumanDuration,
    mqtt_packet_limit::MqttPacketLimit,
    notification_body_limit::NotificationBodyLimit,
    schema::{durable_queue::DurableQueue, private_addresses::PrivateAddresses},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{num::NonZeroU32, time::Duration};
use strum::{Display, EnumString};

/// The most notifications `notifications.delivery.max_concurrent` may keep in
/// flight at once.
pub const MAX_CONCURRENT_DELIVERIES_CEILING: u32 = 1_024;

/// The longest failure diagnostic `notifications.delivery.max_error_length`
/// may keep: the longest failure summary the store holds.
pub const MAX_ERROR_LENGTH_CEILING: u32 = 2_048;

/// The most attempts `notifications.delivery.max_attempts` may allow: the
/// largest count the store's signed 32-bit attempt counter holds.
pub const MAX_DELIVERY_ATTEMPTS_CEILING: u32 = 2_147_483_647;

/// The longest claim `notifications.delivery.queue.lease_timeout` may hold:
/// one day, the longest lease term the store accepts.
pub const MAX_DELIVERY_LEASE: Duration = Duration::from_hours(24);

/// How notifications are delivered to HTTP and MQTT subscribers.
///
/// Both deliver every notification; they differ only in durability.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(
    description = "How notifications are delivered to HTTP and MQTT subscribers. Both modes deliver every notification; they differ only in durability."
)]
pub enum DeliveryMode {
    /// Subscriptions are evaluated and notifications delivered in-process,
    /// without persistence, retry, or dead-lettering. A crashed broker loses
    /// only the notifications still in flight. Deliveries a durable broker
    /// already queued in the tenant database are still drained, with their
    /// retry.
    #[default]
    #[schemars(
        description = "Subscriptions are evaluated and notifications sent from memory, without persistence, retry or dead-lettering; a crashed broker loses the notifications still in flight. Deliveries already queued in the tenant database are still drained, with their retries."
    )]
    Live,
    /// Every notification is recorded in the durable queue in the commit that
    /// raised it, and delivered with retry, leasing and dead-lettering.
    #[schemars(
        description = "Every notification is written to the durable queue in the transaction that raised it, and sent by a worker with retry, leasing and dead-lettering."
    )]
    Durable,
}

/// How long a finished delivery is kept.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(description = "How long finished deliveries and their change events are kept in the tenant database.")]
pub struct DeliveryRetentionConfig {
    /// How long an acknowledged delivery is kept, in human-readable form
    /// (`"1day"`).
    #[schemars(description = "How long a delivered notification is kept.")]
    pub acknowledged_after: HumanDuration,
    /// How long a delivery that ran out of attempts is kept, in
    /// human-readable form (`"7days"`): longer than an acknowledged one, as
    /// evidence of a notification that never arrived.
    #[schemars(description = "How long a delivery that ran out of attempts is kept, as a record of a notification that never arrived.")]
    pub dead_letter_after: HumanDuration,
    /// How long an event no delivery refers to is kept, in human-readable
    /// form (`"1day"`).
    #[schemars(description = "How long a change event no delivery refers to any more is kept.")]
    pub orphan_event_after: HumanDuration,
    /// How often expired deliveries and events are removed, in human-readable
    /// form (`"5m"`). A pass runs only when the tenant may have written since
    /// the last one.
    #[schemars(description = "How often expired deliveries and events are removed. A pass runs only for a tenant that may have written since the last one.")]
    pub sweep_interval: HumanDuration,
}

impl Default for DeliveryRetentionConfig {
    fn default() -> DeliveryRetentionConfig {
        DeliveryRetentionConfig {
            acknowledged_after: HumanDuration::from_days(1),
            dead_letter_after: HumanDuration::from_days(7),
            orphan_event_after: HumanDuration::from_days(1),
            sweep_interval: HumanDuration::from_secs(300),
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
#[derive(Clone, Deserialize, Serialize, JsonSchema, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(
    description = "How notifications reach their subscribers. The queue and its retention govern the delivery drain, which runs under either mode: a tenant database can still hold deliveries queued by a durable broker, or by this one before it was switched to live delivery."
)]
pub struct DeliveryConfig {
    /// Whether notifications are delivered `live` (in-process) or `durable`
    /// (queued).
    #[schemars(description = "Whether notifications are sent from memory (`\"live\"`) or through a queue in the tenant database (`\"durable\"`).")]
    pub mode: DeliveryMode,
    /// How long an outbound notification may take before it is abandoned, in
    /// human-readable form (`"10s"`). A subscription's own Table 5.2.15-1
    /// `timeout` may only shorten it.
    #[schemars(
        description = "The longest an outgoing notification may take before it is abandoned, live or queued. A subscription's own `timeout` can shorten it but not lengthen it."
    )]
    pub request_timeout: HumanDuration,
    /// The most queued notifications sent at once. A delivery pass keeps this
    /// many in flight for as long as the queue holds due deliveries, claiming
    /// one each time an attempt is acknowledged.
    #[schemars(
        description = "The most notifications sent at once. A delivery pass keeps this many in flight for as long as the queue holds due deliveries, so a slow or unreachable endpoint holds only the slots its own attempts occupy."
    )]
    pub max_concurrent: NonZeroU32,
    /// The most characters of a failure diagnostic kept when a failed attempt
    /// is recorded; at most the longest failure summary the store keeps.
    #[schemars(
        description = "The most characters of a failure diagnostic kept with a failed delivery attempt. It may be raised to the longest failure summary the store keeps."
    )]
    pub max_error_length: NonZeroU32,
    /// Whether an HTTP endpoint or an MQTT broker resolving into private or
    /// reserved address space is `allowed` or `refused`. Checked against the
    /// address actually resolved.
    #[schemars(
        description = "Whether a notification endpoint, an HTTP URL or an MQTT broker, resolving into private or reserved address space is notified. Checked against the address actually resolved, not the host name. `allowed_addresses` exempts addresses from a `refused`, and `denied_addresses` refuses addresses whatever this is set to."
    )]
    pub private_addresses: PrivateAddresses,
    /// Addresses and subnets a notification endpoint may resolve into even
    /// when `private_addresses` is `refused`.
    #[schemars(
        description = "IP addresses and subnets (`\"10.2.0.0/24\"`, `\"10.3.0.17\"`, `\"fd00:1234::/48\"`) that a notification endpoint, an HTTP URL or an MQTT broker, may resolve into even though `private_addresses` is `refused`. Empty by default. An entry in `denied_addresses` takes precedence."
    )]
    pub allowed_addresses: Vec<AddressRule>,
    /// Addresses and subnets a notification endpoint is never notified at.
    #[schemars(
        description = "IP addresses and subnets (`\"203.0.113.0/24\"`, `\"198.51.100.9\"`) that a notification endpoint, an HTTP URL or an MQTT broker, must not resolve into, public ones included and whatever `private_addresses` is set to. Empty by default. Takes precedence over `allowed_addresses`."
    )]
    pub denied_addresses: Vec<AddressRule>,
    /// The largest notification body an HTTP delivery sends, in human-readable
    /// form (`"128 MiB"`). A larger notification fails without a retry.
    #[schemars(
        description = "The largest notification body sent over HTTP. A larger notification is not sent, and its delivery fails without a retry, since every attempt would send the same body. A notification carries the entities of a change in full, so a limit at `request_limits.max_body` or below could refuse to notify an entity one request wrote."
    )]
    pub max_http_body: NotificationBodyLimit,
    /// The largest MQTT packet a notification is published in, from one byte
    /// to 256 MiB, in human-readable form (`"128 MiB"`). It is also the largest
    /// packet the broker reads. An MQTT 5 server that announces a smaller
    /// maximum lowers it for that connection. A larger notification fails
    /// without a retry.
    #[schemars(
        description = "The largest MQTT packet a notification is published in, envelope and topic included. It is also the largest packet the broker accepts from an MQTT server. When an MQTT 5 server announces a smaller maximum, that maximum applies on its connection. A larger notification is not published, and its delivery fails without a retry. A notification carries the entities of a change in full, so a limit at `request_limits.max_body` or below could refuse to notify an entity one request wrote. No packet is longer than 256 MiB, so `request_limits.max_body` stays below that."
    )]
    pub max_mqtt_packet: MqttPacketLimit,
    /// The attempts a queued delivery gets before it is dead-lettered.
    #[schemars(
        description = "The attempts a queued notification gets. A delivery that has failed this many times is dead-lettered and kept for `notifications.delivery.retention.dead_letter_after`, as a record of a notification that never arrived."
    )]
    pub max_attempts: NonZeroU32,
    /// How the queue is polled, claimed and retried. A delivery is retried on
    /// a delay that doubles from `retry_interval` up to `max_retry_interval`.
    #[schemars(
        description = "How the delivery queue is polled, claimed and retried. A failed delivery is retried on a delay that doubles from `notifications.delivery.queue.retry_interval` up to `notifications.delivery.queue.max_retry_interval`."
    )]
    pub queue: DurableQueue,
    /// How long a finished delivery is kept.
    #[schemars(description = "How long finished deliveries and their change events are kept.")]
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
            allowed_addresses: Vec::new(),
            denied_addresses: Vec::new(),
            max_http_body: NotificationBodyLimit::DEFAULT,
            max_mqtt_packet: MqttPacketLimit::DEFAULT,
            max_attempts: NonZeroU32::MIN.saturating_add(4),
            queue: DurableQueue {
                retry_interval: HumanDuration::from_secs(5),
                ..DurableQueue::default()
            },
            retention: DeliveryRetentionConfig::default(),
        }
    }
}

impl DeliveryConfig {
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
    use crate::schema::{
        notifications::delivery::{DeliveryConfig, DeliveryMode},
        private_addresses::PrivateAddresses,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn the_defaults_deliver_live_to_any_address() {
        let config = DeliveryConfig::default();
        assert_eq!(config.mode, DeliveryMode::Live);
        assert_eq!(config.private_addresses, PrivateAddresses::Allowed);
    }

    #[test]
    fn the_default_notification_limits_admit_a_body_twice_the_default_request_body() {
        let config = DeliveryConfig::default();
        assert_eq!(config.max_http_body.octets(), 134_217_728);
        assert_eq!(config.max_mqtt_packet.octets(), 134_217_728);
    }
}
