//! The largest MQTT packet a notification may go out in.

use crate::human_byte_size::HumanByteSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A packet limit no MQTT connection can enforce.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum MqttPacketLimitError {
    /// A zero limit refuses every packet, the connect handshake included.
    #[error("notifications.delivery.max_mqtt_packet must be above zero")]
    Zero,
    /// The remaining length of an MQTT packet is a variable byte integer of
    /// at most four bytes, so no packet is longer than 256 MiB (MQTT 3.1.1
    /// section 2.2.3, MQTT 5.0 section 1.5.5).
    #[error("notifications.delivery.max_mqtt_packet of {0} bytes is above the 256 MiB the protocol carries")]
    AboveMaximum(u64),
}

/// The largest MQTT packet, measured whole, a notification may be published
/// in, from one byte to 256 MiB.
///
/// It bounds what the broker writes and what it advertises it reads, on MQTT
/// 3.1.1 and MQTT 5 alike. Its wire form is a byte size (`"128 MiB"`).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(try_from = "HumanByteSize", into = "HumanByteSize")]
#[schemars(description = "An MQTT packet size, written as a byte size from 1 byte to `\"256 MiB\"`.")]
pub struct MqttPacketLimit(u32);

impl MqttPacketLimit {
    /// The largest limit, 256 MiB.
    pub const MAXIMUM: MqttPacketLimit = MqttPacketLimit(256 * 1024 * 1024);

    /// The default limit, 128 MiB: room for a notification of an entity as
    /// large as the default request body, with its envelope.
    pub const DEFAULT: MqttPacketLimit = MqttPacketLimit(128 * 1024 * 1024);

    /// The limit in octets.
    #[must_use]
    pub const fn octets(self) -> u32 {
        self.0
    }
}

impl TryFrom<HumanByteSize> for MqttPacketLimit {
    type Error = MqttPacketLimitError;

    fn try_from(size: HumanByteSize) -> Result<MqttPacketLimit, MqttPacketLimitError> {
        match size.as_u64() {
            0 => Err(MqttPacketLimitError::Zero),
            octets => u32::try_from(octets)
                .ok()
                .filter(|octets| *octets <= MqttPacketLimit::MAXIMUM.0)
                .map(MqttPacketLimit)
                .ok_or(MqttPacketLimitError::AboveMaximum(octets)),
        }
    }
}

impl From<MqttPacketLimit> for HumanByteSize {
    fn from(limit: MqttPacketLimit) -> HumanByteSize {
        HumanByteSize::b(u64::from(limit.0))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        human_byte_size::HumanByteSize,
        mqtt_packet_limit::{MqttPacketLimit, MqttPacketLimitError},
    };
    use pretty_assertions::assert_eq;
    use serde::{Deserialize, Serialize};
    use std::error::Error;

    #[derive(Debug, Deserialize, Serialize)]
    struct Holder {
        limit: MqttPacketLimit,
    }

    #[test]
    fn a_zero_packet_limit_is_refused() {
        assert_eq!(MqttPacketLimit::try_from(HumanByteSize::b(0)), Err(MqttPacketLimitError::Zero));
    }

    #[test]
    fn a_packet_limit_of_one_byte_and_of_256_mib_is_accepted() -> Result<(), MqttPacketLimitError> {
        assert_eq!(MqttPacketLimit::try_from(HumanByteSize::b(1))?.octets(), 1);
        assert_eq!(MqttPacketLimit::try_from(HumanByteSize::mib(256))?, MqttPacketLimit::MAXIMUM);
        Ok(())
    }

    #[test]
    fn a_packet_limit_over_256_mib_is_refused() {
        for octets in [268_435_457, 4_294_967_296, u64::MAX] {
            assert_eq!(
                MqttPacketLimit::try_from(HumanByteSize::b(octets)),
                Err(MqttPacketLimitError::AboveMaximum(octets))
            );
        }
    }

    #[test]
    fn the_default_packet_limit_is_128_mib() {
        assert_eq!(MqttPacketLimit::DEFAULT.octets(), 134_217_728);
    }

    #[test]
    fn a_packet_limit_parses_a_human_size_and_renders_one() -> Result<(), Box<dyn Error>> {
        let holder: Holder = toml::from_str("limit = \"16 MiB\"")?;
        assert_eq!(holder.limit.octets(), 16_777_216);
        let rendered = toml::to_string(&holder)?;
        assert!(rendered.contains("limit = \"16 MiB\""), "{rendered}");
        Ok(())
    }

    #[test]
    fn a_packet_limit_out_of_range_fails_to_parse() {
        let outcome = toml::from_str::<Holder>("limit = \"512 MiB\"");
        assert!(outcome.is_err(), "{outcome:?}");
    }
}
