//! How the MQTT connection pool is tuned.

use std::time::Duration;

/// Default number of broker connections held open.
const DEFAULT_MAX_CONNECTIONS: usize = 100;

/// Default time allowed for a broker to complete its half of the connect.
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Default MQTT keep-alive interval.
const DEFAULT_KEEPALIVE: Duration = Duration::from_secs(30);

/// Default interval between sweeps for connections that have gone quiet.
const DEFAULT_HEALTH_PROBE_INTERVAL: Duration = Duration::from_secs(30);

/// Default time a QoS 1/2 publish waits for the broker's acknowledgement before
/// it is treated as an unconfirmed, retryable delivery.
const DEFAULT_PUBLISH_ACK_TIMEOUT: Duration = Duration::from_secs(30);

/// Shortest wait before retrying a broker that refused a connection.
const RECONNECT_MIN_DELAY: Duration = Duration::from_secs(1);

/// Longest wait before retrying a broker that refused a connection.
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(60);

/// Multiplier applied to the reconnect delay after each failure.
const RECONNECT_FACTOR: f32 = 2.0;

/// MQTT connection pool configuration.
#[derive(Debug, Clone)]
pub struct MqttPoolConfig {
    /// How many broker connections may be held open at once.
    pub max_connections: usize,
    /// How long a broker has to complete its half of the connect.
    pub connect_timeout: Duration,
    /// The MQTT keep-alive interval.
    pub keepalive: Duration,
    /// The backoff applied between reconnection attempts.
    pub reconnect_backoff: backon::ExponentialBuilder,
    /// How often connections are swept for having gone quiet.
    pub health_probe_interval: Duration,
    /// How long a QoS 1/2 publish waits for the broker's acknowledgement before
    /// it is treated as unconfirmed and retryable.
    pub publish_ack_timeout: Duration,
}

impl Default for MqttPoolConfig {
    fn default() -> MqttPoolConfig {
        MqttPoolConfig {
            max_connections: DEFAULT_MAX_CONNECTIONS,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            keepalive: DEFAULT_KEEPALIVE,
            reconnect_backoff: backon::ExponentialBuilder::default()
                .with_min_delay(RECONNECT_MIN_DELAY)
                .with_max_delay(RECONNECT_MAX_DELAY)
                .with_factor(RECONNECT_FACTOR)
                .with_jitter()
                .without_max_times(),
            health_probe_interval: DEFAULT_HEALTH_PROBE_INTERVAL,
            publish_ack_timeout: DEFAULT_PUBLISH_ACK_TIMEOUT,
        }
    }
}
