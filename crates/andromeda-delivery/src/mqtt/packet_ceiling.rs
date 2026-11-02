//! The largest packet one broker connection writes, and which side set it.

use crate::oversized_notification::OversizedNotification;
use andromeda_configuration::mqtt_packet_limit::MqttPacketLimit;

/// The largest packet one broker connection writes, with where the bound came
/// from.
///
/// An MQTT 5 server may announce in its CONNACK the largest packet it accepts,
/// and a client sends nothing larger (MQTT 5.0 section 3.2.2.3.6). The
/// connection writes the smaller of that and the configured limit, and keeps
/// which one it was so a refusal names the limit an operator can act on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqttPacketCeiling {
    /// `notifications.delivery.max_mqtt_packet`: the server announced no
    /// smaller maximum, or speaks MQTT 3.1.1, which announces none.
    Configured(MqttPacketLimit),
    /// The server announced a maximum below
    /// `notifications.delivery.max_mqtt_packet`.
    ServerAnnounced {
        /// The maximum packet size the CONNACK carried, in bytes.
        announced: u32,
        /// The configured limit the announced maximum undercuts.
        configured: MqttPacketLimit,
    },
}

impl MqttPacketCeiling {
    /// The ceiling of a connection configured with `configured` whose server
    /// announced `announced`, or announced nothing.
    ///
    /// An announced maximum equal to or above the configured limit leaves the
    /// configured limit in force, so it is the one a refusal names.
    #[must_use]
    pub const fn negotiated(configured: MqttPacketLimit, announced: Option<u32>) -> MqttPacketCeiling {
        match announced {
            Some(announced) if announced < configured.octets() => MqttPacketCeiling::ServerAnnounced { announced, configured },
            Some(_) | None => MqttPacketCeiling::Configured(configured),
        }
    }

    /// The ceiling in bytes.
    #[must_use]
    pub const fn octets(self) -> u32 {
        match self {
            MqttPacketCeiling::Configured(limit) => limit.octets(),
            MqttPacketCeiling::ServerAnnounced { announced, .. } => announced,
        }
    }

    /// Admit a packet of `size` bytes.
    ///
    /// # Errors
    ///
    /// Returns the [`OversizedNotification`] naming this ceiling's source when
    /// `size` exceeds it.
    pub const fn admit(self, size: usize) -> Result<(), OversizedNotification> {
        if size <= self.octets() as usize {
            return Ok(());
        }
        Err(match self {
            MqttPacketCeiling::Configured(limit) => OversizedNotification::MaxMqttPacket { size, limit: limit.octets() },
            MqttPacketCeiling::ServerAnnounced { announced, configured } => OversizedNotification::MqttServerAnnounced {
                size,
                announced,
                configured: configured.octets(),
            },
        })
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{mqtt::packet_ceiling::MqttPacketCeiling, oversized_notification::OversizedNotification};
    use andromeda_configuration::{human_byte_size::HumanByteSize, mqtt_packet_limit::MqttPacketLimit};
    use pretty_assertions::assert_eq;

    fn configured_mib(mib: u64) -> MqttPacketLimit {
        MqttPacketLimit::try_from(HumanByteSize::mib(mib)).expect("a limit within the protocol")
    }

    #[test]
    fn an_announced_maximum_below_the_configured_limit_bounds_the_connection() {
        let ceiling = MqttPacketCeiling::negotiated(configured_mib(128), Some(2_000_000));
        assert_eq!(
            ceiling,
            MqttPacketCeiling::ServerAnnounced {
                announced: 2_000_000,
                configured: configured_mib(128),
            }
        );
        assert_eq!(ceiling.octets(), 2_000_000);
    }

    #[test]
    fn an_announced_maximum_at_or_above_the_configured_limit_leaves_the_configured_limit() {
        let configured = configured_mib(1);
        for announced in [Some(1_048_576), Some(268_435_455), None] {
            assert_eq!(MqttPacketCeiling::negotiated(configured, announced), MqttPacketCeiling::Configured(configured));
        }
    }

    #[test]
    fn a_packet_at_the_ceiling_is_admitted() {
        assert_eq!(MqttPacketCeiling::negotiated(configured_mib(1), None).admit(1_048_576), Ok(()));
        assert_eq!(MqttPacketCeiling::negotiated(configured_mib(128), Some(2_000_000)).admit(2_000_000), Ok(()));
    }

    #[test]
    fn a_packet_over_the_announced_maximum_is_refused_naming_the_server_and_the_configured_limit() {
        let refused = MqttPacketCeiling::negotiated(configured_mib(128), Some(2_000_000)).admit(8_389_548);
        assert_eq!(
            refused,
            Err(OversizedNotification::MqttServerAnnounced {
                size: 8_389_548,
                announced: 2_000_000,
                configured: 134_217_728,
            })
        );
    }

    #[test]
    fn a_packet_over_the_configured_limit_is_refused_naming_max_mqtt_packet() {
        let refused = MqttPacketCeiling::negotiated(configured_mib(1), Some(2_000_000)).admit(1_048_577);
        assert_eq!(
            refused,
            Err(OversizedNotification::MaxMqttPacket {
                size: 1_048_577,
                limit: 1_048_576
            })
        );
    }
}
