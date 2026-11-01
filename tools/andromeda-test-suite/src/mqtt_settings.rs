//! The MQTT brokers of a stack that runs them, as the suite configuration names them.

use crate::{config_error::ConfigError, host_name::HostName};
use serde::Deserialize;
use url::Host;

/// The MQTT brokers notifications are published to.
#[derive(Debug, Clone, Deserialize)]
pub struct MqttSettings {
    /// The anonymous Mosquitto broker, as the brokers and the test process reach it.
    host: HostName,
    /// The Mosquitto broker refusing anonymous clients on the ports endpoints name.
    auth_host: HostName,
    /// The anonymous Mosquitto broker whose packet limit admits a notification of several MiB, on a stack that runs it.
    #[serde(default)]
    large_host: Option<HostName>,
    /// The listener port other than 1883 both Mosquitto brokers serve.
    non_default_port: u16,
    /// The listener without a mount point an observer subscribes on, so that every topic carries the port it was
    /// published through.
    observer_port: u16,
}

impl MqttSettings {
    /// The anonymous Mosquitto broker.
    #[must_use]
    pub const fn host(&self) -> &Host<String> {
        self.host.host()
    }

    /// The Mosquitto broker that requires credentials.
    #[must_use]
    pub const fn auth_host(&self) -> &Host<String> {
        self.auth_host.host()
    }

    /// The Mosquitto broker that admits a notification of several MiB.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] when the stack runs no such broker.
    pub fn large_host(&self) -> Result<&Host<String>, ConfigError> {
        self.large_host.as_ref().map(HostName::host).ok_or(ConfigError::missing("MQTT", "LARGE_HOST"))
    }

    /// The listener port other than 1883.
    #[must_use]
    pub const fn non_default_port(&self) -> u16 {
        self.non_default_port
    }

    /// The listener an observer subscribes on.
    #[must_use]
    pub const fn observer_port(&self) -> u16 {
        self.observer_port
    }
}
