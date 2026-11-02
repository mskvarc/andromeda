//! A notification refused before sending because it exceeds a size limit,
//! named with the limit that refused it.
//!
//! An MQTT connection writes the smaller of the configured packet limit and the
//! maximum the server announced, so the refusal says which of the two it was:
//! an operator reading a limit absent from the broker's configuration otherwise
//! has no way to tell where the number came from.

use andromeda_telemetry::delivery_ops::NotificationSizeLimit;

/// A notification over the limit of the transport carrying it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OversizedNotification {
    /// The HTTP body exceeds `notifications.delivery.max_http_body`.
    #[error("notification body of {size} bytes exceeds the {limit} bytes notifications.delivery.max_http_body allows")]
    MaxHttpBody {
        /// The body's size, in bytes.
        size: usize,
        /// The configured limit, in bytes.
        limit: usize,
    },
    /// The MQTT packet exceeds `notifications.delivery.max_mqtt_packet`, and the
    /// server announced no smaller maximum.
    #[error("notification packet of {size} bytes exceeds the {limit} bytes notifications.delivery.max_mqtt_packet allows")]
    MaxMqttPacket {
        /// The whole packet's size, in bytes.
        size: usize,
        /// The configured limit, in bytes.
        limit: u32,
    },
    /// The MQTT packet exceeds the maximum packet size the server announced,
    /// which is below `notifications.delivery.max_mqtt_packet` (MQTT 5.0
    /// section 3.2.2.3.6 forbids sending a larger packet).
    #[error(
        "notification packet of {size} bytes exceeds the maximum packet size of {announced} bytes the MQTT server announced \
         (notifications.delivery.max_mqtt_packet is {configured} bytes)"
    )]
    MqttServerAnnounced {
        /// The whole packet's size, in bytes.
        size: usize,
        /// The maximum the server announced in its CONNACK, in bytes.
        announced: u32,
        /// The configured limit the announced maximum undercuts, in bytes.
        configured: u32,
    },
}

impl OversizedNotification {
    /// The limit that refused the notification, as the metric labels it.
    #[must_use]
    pub const fn limit(&self) -> NotificationSizeLimit {
        match self {
            OversizedNotification::MaxHttpBody { .. } => NotificationSizeLimit::MaxHttpBody,
            OversizedNotification::MaxMqttPacket { .. } => NotificationSizeLimit::MaxMqttPacket,
            OversizedNotification::MqttServerAnnounced { .. } => NotificationSizeLimit::MqttServerAnnounced,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::oversized_notification::OversizedNotification;
    use andromeda_telemetry::delivery_ops::NotificationSizeLimit;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_refusal_under_the_announced_maximum_names_both_figures_and_the_server() {
        let refusal = OversizedNotification::MqttServerAnnounced {
            size: 8_389_548,
            announced: 2_000_000,
            configured: 134_217_728,
        };
        assert_eq!(
            refusal.to_string(),
            "notification packet of 8389548 bytes exceeds the maximum packet size of 2000000 bytes the MQTT server announced \
             (notifications.delivery.max_mqtt_packet is 134217728 bytes)"
        );
        assert_eq!(refusal.limit(), NotificationSizeLimit::MqttServerAnnounced);
    }

    #[test]
    fn a_refusal_under_the_configured_packet_limit_names_max_mqtt_packet() {
        let refusal = OversizedNotification::MaxMqttPacket {
            size: 2_097_434,
            limit: 1_048_576,
        };
        assert_eq!(
            refusal.to_string(),
            "notification packet of 2097434 bytes exceeds the 1048576 bytes notifications.delivery.max_mqtt_packet allows"
        );
        assert_eq!(refusal.limit(), NotificationSizeLimit::MaxMqttPacket);
    }

    #[test]
    fn a_refusal_under_the_configured_body_limit_names_max_http_body() {
        let refusal = OversizedNotification::MaxHttpBody { size: 2048, limit: 1024 };
        assert_eq!(
            refusal.to_string(),
            "notification body of 2048 bytes exceeds the 1024 bytes notifications.delivery.max_http_body allows"
        );
        assert_eq!(refusal.limit(), NotificationSizeLimit::MaxHttpBody);
    }
}
