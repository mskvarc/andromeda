//! The `[management]` table: where the broker serves its operational
//! endpoints.
//!
//! The management plane carries `/livez`, `/readyz` and, when
//! `[observability.metrics]` enables them, `/metrics` on a bare middleware
//! stack, isolated from the public NGSI-LD data plane so a Prometheus scraper
//! or a kubelet is never subject to CORS, CSRF, security headers, body limits
//! or compression, and so probes and metrics keep answering while the data
//! plane is saturated or draining.

use crate::schema::listener_transport::ListenerTransport;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use strum::{Display, EnumString};
use thiserror::Error;
use toml_comment::TomlComment;

/// Where, and whether, the broker serves its management endpoints.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum ManagementMode {
    /// No management plane is served; the data-plane probes remain and metrics
    /// cannot be enabled.
    Disabled,
    /// The management endpoints are mounted at the root of the data-plane port,
    /// behind the full inbound stack.
    SharedPort,
    /// The management endpoints bind their own TCP port, isolated from the public
    /// data plane by a bare middleware stack.
    #[default]
    SeparatePort,
}

/// Invalid management-plane configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ManagementConfigError {
    /// The separate management port was zero, which is reserved for dynamic
    /// binding and cannot be advertised as a listener.
    #[error("management.port must be greater than 0 when management.mode is \"separate-port\"")]
    ZeroPort,
    /// The separate management port was set to the data-plane port. A wildcard
    /// host overlaps every host, so the two planes are compared on port alone.
    #[error("management.port ({port}) must differ from server.port when management.mode is \"separate-port\"")]
    PortCollision {
        /// The port both planes were configured to bind.
        port: u16,
    },
    /// Metrics were enabled while no management plane exists to serve them.
    #[error(
        "observability.metrics.mode is \"prometheus\" but management.mode is \"disabled\"; \
         set management.mode to \"separate-port\" or \"shared-port\", or disable metrics"
    )]
    MetricsWithoutManagementPlane,
}

/// The management plane's topology and listener.
///
/// `host`, `port` and `transport` are read only in [`ManagementMode::SeparatePort`]; in
/// [`ManagementMode::SharedPort`] the endpoints ride the data-plane listener, and in
/// [`ManagementMode::Disabled`] no management listener is bound.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct ManagementConfig {
    /// Where the management endpoints are served: `disabled`, `shared-port` or
    /// `separate-port`.
    #[toml_comment(inline)]
    pub mode: ManagementMode,
    /// Management listener address. Read only when `mode` is `separate-port`.
    #[toml_comment(inline)]
    pub host: IpAddr,
    /// Management listener port. Read only when `mode` is `separate-port`.
    #[toml_comment(inline)]
    pub port: u16,
    /// Whether the management listener serves plaintext or terminates TLS with
    /// the `[tls]` identity: `plaintext` or `tls`. Read only when `mode` is
    /// `separate-port`.
    #[toml_comment(inline)]
    pub transport: ListenerTransport,
}

impl Default for ManagementConfig {
    // Manual rather than derived: the wildcard host and the 1086 default port are
    // concrete values a field-wise derive cannot express.
    fn default() -> ManagementConfig {
        ManagementConfig {
            mode: ManagementMode::SeparatePort,
            host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 1086,
            transport: ListenerTransport::Plaintext,
        }
    }
}

impl ManagementConfig {
    /// Validate the management-plane configuration in isolation.
    ///
    /// # Errors
    ///
    /// Returns [`ManagementConfigError::ZeroPort`] when a `separate-port` plane is
    /// configured on port zero.
    pub const fn validate(&self) -> Result<(), ManagementConfigError> {
        match self.mode {
            ManagementMode::SeparatePort if self.port == 0 => Err(ManagementConfigError::ZeroPort),
            ManagementMode::SeparatePort | ManagementMode::SharedPort | ManagementMode::Disabled => Ok(()),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::{
        listener_transport::ListenerTransport,
        management::{ManagementConfig, ManagementConfigError, ManagementMode},
    };
    use std::str::FromStr;

    #[test]
    fn the_default_plane_binds_port_1086_in_plaintext() {
        let config = ManagementConfig::default();
        pretty_assertions::assert_eq!(config.mode, ManagementMode::SeparatePort);
        pretty_assertions::assert_eq!(config.port, 1086);
        pretty_assertions::assert_eq!(config.transport, ListenerTransport::Plaintext);
    }

    #[test]
    fn mode_round_trips_kebab_case_through_serde_and_from_str() {
        for (mode, token) in [
            (ManagementMode::Disabled, "disabled"),
            (ManagementMode::SharedPort, "shared-port"),
            (ManagementMode::SeparatePort, "separate-port"),
        ] {
            assert_eq!(serde_json::to_string(&mode).expect("serialises"), format!("\"{token}\""));
            assert_eq!(ManagementMode::from_str(token).expect("parses"), mode);
        }
        assert!(serde_json::from_str::<ManagementMode>("\"merged\"").is_err());
    }

    #[test]
    fn separate_port_zero_is_rejected() {
        let config = ManagementConfig {
            port: 0,
            ..ManagementConfig::default()
        };
        pretty_assertions::assert_eq!(config.validate(), Err(ManagementConfigError::ZeroPort));
    }

    #[test]
    fn a_plane_without_its_own_listener_ignores_port_zero() {
        for mode in [ManagementMode::Disabled, ManagementMode::SharedPort] {
            let config = ManagementConfig {
                mode,
                port: 0,
                ..ManagementConfig::default()
            };
            assert!(config.validate().is_ok());
        }
    }
}
