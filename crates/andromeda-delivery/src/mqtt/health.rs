//! How healthy one pooled broker connection is.

use crate::endpoint::mqtt::MqttVersion;
use std::sync::atomic::{AtomicU8, Ordering};

/// Health of a single pooled MQTT connection.
///
/// `Degraded` is not a failure: it means nothing has arrived from the broker
/// for twice the keep-alive interval, which is suspicious but not yet a reason
/// to tear the connection down. Only `Unhealthy` — the event loop reporting an
/// error, or a publish failing — takes a connection out of the fast path and
/// puts it behind the reconnect backoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum ConnectionHealth {
    /// The broker is answering.
    Healthy,
    /// Nothing has been heard for longer than expected.
    Degraded,
    /// The connection has failed.
    Unhealthy,
}

impl ConnectionHealth {
    /// The value stored in the atomic cell.
    const fn as_bits(self) -> u8 {
        match self {
            ConnectionHealth::Healthy => 0,
            ConnectionHealth::Degraded => 1,
            ConnectionHealth::Unhealthy => 2,
        }
    }

    /// The health a stored value names.
    ///
    /// Total over `u8` because [`HealthCell`] is the only writer and the
    /// conversion has to be infallible to be readable without a lock: any bit
    /// pattern nothing wrote is reported as `Unhealthy`, which is the safe
    /// direction for a value that decides whether to publish.
    const fn from_bits(bits: u8) -> ConnectionHealth {
        match bits {
            0 => ConnectionHealth::Healthy,
            1 => ConnectionHealth::Degraded,
            _unwritten => ConnectionHealth::Unhealthy,
        }
    }
}

/// One connection's health, readable from the delivery path without a lock.
///
/// The event loop writes it on every packet, so a mutex here would be taken
/// once per inbound message on a connection that may be carrying `$SYS`
/// traffic.
#[derive(Debug)]
pub struct HealthCell(AtomicU8);

impl HealthCell {
    /// A cell holding the given health.
    pub const fn new(health: ConnectionHealth) -> HealthCell {
        HealthCell(AtomicU8::new(health.as_bits()))
    }

    /// The current health.
    pub fn load(&self) -> ConnectionHealth {
        ConnectionHealth::from_bits(self.0.load(Ordering::Acquire))
    }

    /// Record the current health.
    pub fn store(&self, health: ConnectionHealth) {
        self.0.store(health.as_bits(), Ordering::Release);
    }
}

/// Summary of a single MQTT connection's health state.
#[derive(Debug, Clone)]
pub struct MqttConnectionHealthSummary {
    /// Broker hostname.
    pub host: String,
    /// Broker port.
    pub port: u16,
    /// MQTT protocol version.
    pub version: MqttVersion,
    /// Current health of this connection.
    pub health: ConnectionHealth,
    /// Seconds elapsed since this connection was last known healthy.
    pub seconds_since_healthy: u64,
    /// Broker uptime from `$SYS/broker/uptime`, if the broker publishes it.
    pub broker_uptime: Option<String>,
    /// Broker version from `$SYS/broker/version`, if the broker publishes it.
    pub broker_version: Option<String>,
    /// Connected client count from `$SYS/broker/clients/connected`, if the
    /// broker publishes it.
    pub clients_connected: Option<String>,
}

#[cfg(test)]
mod tests {
    use crate::mqtt::health::{ConnectionHealth, HealthCell};

    #[test]
    fn each_health_renders_the_label_the_health_response_carries() {
        pretty_assertions::assert_eq!(ConnectionHealth::Healthy.to_string(), "healthy");
        pretty_assertions::assert_eq!(ConnectionHealth::Degraded.to_string(), "degraded");
        pretty_assertions::assert_eq!(ConnectionHealth::Unhealthy.to_string(), "unhealthy");
    }

    #[test]
    fn every_health_round_trips_through_the_atomic_cell() {
        let cell = HealthCell::new(ConnectionHealth::Healthy);
        for health in [ConnectionHealth::Healthy, ConnectionHealth::Degraded, ConnectionHealth::Unhealthy] {
            cell.store(health);
            pretty_assertions::assert_eq!(cell.load(), health);
        }
    }

    #[test]
    fn a_value_nothing_wrote_reads_as_unhealthy_rather_than_as_healthy() {
        pretty_assertions::assert_eq!(ConnectionHealth::from_bits(200), ConnectionHealth::Unhealthy);
    }
}
