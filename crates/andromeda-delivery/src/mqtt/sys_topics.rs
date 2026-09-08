//! Broker metadata collected from `$SYS` topics.
//!
//! Not part of delivery: the broker publishes these and Andromeda subscribes
//! to them so that `/health` can say something about the far end of the
//! connection rather than only about its own socket. Subscribing is
//! fire-and-forget — a broker that does not publish `$SYS`, or does not allow
//! subscribing to it, simply reports nothing.

use strum::EnumIter;

/// A `$SYS` metric topic this broker subscribes to.
///
/// A closed enum rather than a list of topic strings: the set of metrics read
/// is fixed, so modelling it this way makes the topic-to-field mapping total and
/// removes the string comparison that decided it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter)]
pub enum SysTopic {
    /// `$SYS/broker/uptime`.
    BrokerUptime,
    /// `$SYS/broker/version`.
    BrokerVersion,
    /// `$SYS/broker/clients/connected`.
    ClientsConnected,
}

impl SysTopic {
    /// The `$SYS` topic string this metric is published on, for the subscribe
    /// call that takes one.
    #[must_use]
    pub const fn topic(self) -> &'static str {
        match self {
            SysTopic::BrokerUptime => "$SYS/broker/uptime",
            SysTopic::BrokerVersion => "$SYS/broker/version",
            SysTopic::ClientsConnected => "$SYS/broker/clients/connected",
        }
    }

    /// The metric a published `$SYS` topic names, or `None` for a topic this
    /// broker does not read.
    #[must_use]
    pub fn from_topic(topic: &str) -> Option<SysTopic> {
        match topic {
            "$SYS/broker/uptime" => Some(SysTopic::BrokerUptime),
            "$SYS/broker/version" => Some(SysTopic::BrokerVersion),
            "$SYS/broker/clients/connected" => Some(SysTopic::ClientsConnected),
            _unsubscribed => None,
        }
    }
}

/// Broker metadata collected from `$SYS` topics.
///
/// The values are free-form telemetry text as the broker publishes them — a
/// human-readable uptime, a version banner, a client count — so they stay
/// `String` rather than being parsed into a shape this broker never reasons
/// about.
#[derive(Debug, Default)]
pub struct SysTopicData {
    /// Value of `$SYS/broker/uptime`.
    pub broker_uptime: Option<String>,
    /// Value of `$SYS/broker/version`.
    pub broker_version: Option<String>,
    /// Value of `$SYS/broker/clients/connected`.
    pub clients_connected: Option<String>,
}

impl SysTopicData {
    /// Record a `$SYS` publish under the metric it belongs to.
    pub fn record(&mut self, topic: SysTopic, value: String) {
        match topic {
            SysTopic::BrokerUptime => self.broker_uptime = Some(value),
            SysTopic::BrokerVersion => self.broker_version = Some(value),
            SysTopic::ClientsConnected => self.clients_connected = Some(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::mqtt::sys_topics::{SysTopic, SysTopicData};
    use strum::IntoEnumIterator;

    #[test]
    fn every_metric_records_into_its_own_field() {
        let mut data = SysTopicData::default();
        data.record(SysTopic::BrokerUptime, "12 seconds".to_owned());
        data.record(SysTopic::BrokerVersion, "mosquitto 2.0".to_owned());
        data.record(SysTopic::ClientsConnected, "3".to_owned());

        pretty_assertions::assert_eq!(data.broker_uptime.as_deref(), Some("12 seconds"));
        pretty_assertions::assert_eq!(data.broker_version.as_deref(), Some("mosquitto 2.0"));
        pretty_assertions::assert_eq!(data.clients_connected.as_deref(), Some("3"));
    }

    #[test]
    fn a_topic_outside_the_known_set_maps_to_no_metric() {
        assert!(SysTopic::from_topic("$SYS/broker/messages/sent").is_none());
    }

    #[test]
    fn every_metric_topic_round_trips_through_from_topic() {
        for sys in SysTopic::iter() {
            pretty_assertions::assert_eq!(SysTopic::from_topic(sys.topic()), Some(sys));
        }
    }
}
