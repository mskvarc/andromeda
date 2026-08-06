//! Broker metadata collected from `$SYS` topics.
//!
//! Not part of delivery: the broker publishes these and Andromeda subscribes
//! to them so that `/health` can say something about the far end of the
//! connection rather than only about its own socket. Subscribing is
//! fire-and-forget — a broker that does not publish `$SYS`, or does not allow
//! subscribing to it, simply reports nothing.

/// `$SYS` topics subscribed to for broker metadata.
pub const SYS_TOPICS: &[&str] = &["$SYS/broker/uptime", "$SYS/broker/version", "$SYS/broker/clients/connected"];

/// Broker metadata collected from `$SYS` topics.
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
    /// Record a `$SYS` publish, ignoring topics this broker does not read.
    pub fn record(&mut self, topic: &str, payload: String) {
        match topic {
            "$SYS/broker/uptime" => self.broker_uptime = Some(payload),
            "$SYS/broker/version" => self.broker_version = Some(payload),
            "$SYS/broker/clients/connected" => self.clients_connected = Some(payload),
            _unsubscribed => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::mqtt::sys_topics::{SYS_TOPICS, SysTopicData};

    #[test]
    fn every_subscribed_topic_lands_in_its_own_field() {
        let mut data = SysTopicData::default();
        data.record("$SYS/broker/uptime", "12 seconds".to_owned());
        data.record("$SYS/broker/version", "mosquitto 2.0".to_owned());
        data.record("$SYS/broker/clients/connected", "3".to_owned());

        pretty_assertions::assert_eq!(data.broker_uptime.as_deref(), Some("12 seconds"));
        pretty_assertions::assert_eq!(data.broker_version.as_deref(), Some("mosquitto 2.0"));
        pretty_assertions::assert_eq!(data.clients_connected.as_deref(), Some("3"));
    }

    #[test]
    fn a_topic_outside_the_subscribed_set_is_ignored() {
        let mut data = SysTopicData::default();
        data.record("$SYS/broker/messages/sent", "9000".to_owned());
        assert!(data.broker_uptime.is_none() && data.broker_version.is_none() && data.clients_connected.is_none());
    }

    #[test]
    fn every_topic_subscribed_to_is_one_the_recorder_reads() {
        for topic in SYS_TOPICS {
            let mut data = SysTopicData::default();
            data.record(topic, "value".to_owned());
            assert!(
                data.broker_uptime.is_some() || data.broker_version.is_some() || data.clients_connected.is_some(),
                "{topic} is subscribed to and then discarded"
            );
        }
    }
}
