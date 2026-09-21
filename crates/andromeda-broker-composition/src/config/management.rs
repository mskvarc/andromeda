//! Management-plane topology and metrics-exposure configuration.
//!
//! The management plane carries observability endpoints (`/livez`, `/readyz`,
//! and, when enabled, `/metrics`) on a bare middleware stack, isolated from the
//! public NGSI-LD data plane so a Prometheus scraper or a kubelet is never
//! subject to CORS, CSRF, helmet, body-limit or compression, and so probes and
//! metrics keep answering while the data plane is saturated or draining.

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use strum::{Display, EnumString};
use thiserror::Error;
use toml_comment::TomlComment;

/// Where, and whether, the broker serves its observability endpoints.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum ManagementMode {
    /// The management endpoints bind their own TCP port, isolated from the public
    /// data plane by a bare middleware stack.
    #[default]
    SeparatePort,
    /// The management endpoints are mounted at the root of the data-plane port,
    /// behind the full inbound stack. The documented cost of a single port.
    Merged,
    /// No management plane is served; the data-plane probes remain and metrics
    /// cannot be enabled.
    Disabled,
}

/// Whether, and in which format, the broker exposes its metrics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum MetricsExposure {
    /// No recorder is installed and no scrape endpoint is mounted.
    #[default]
    Disabled,
    /// A Prometheus recorder is installed and served at `GET /metrics` on the
    /// management plane.
    Prometheus,
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
        "management.metrics is \"prometheus\" but management.mode is \"disabled\"; \
         set management.mode to \"separate-port\" or \"merged\", or disable metrics"
    )]
    MetricsWithoutManagementPlane,
}

/// Management-plane configuration.
///
/// `host` and `port` are read only in [`ManagementMode::SeparatePort`]; in
/// [`ManagementMode::Merged`] the endpoints ride the data-plane listener, and in
/// [`ManagementMode::Disabled`] no management listener is bound.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct ManagementConfig {
    /// Management-plane topology.
    #[toml_comment(inline)]
    pub mode: ManagementMode,
    /// Management listener address. Ignored unless `mode` is `separate-port`.
    #[toml_comment(inline)]
    pub host: IpAddr,
    /// Management listener port. Ignored unless `mode` is `separate-port`.
    #[toml_comment(inline)]
    pub port: u16,
    /// Metrics exposure mode.
    #[toml_comment(inline)]
    pub metrics: MetricsExposure,
}

impl Default for ManagementConfig {
    // Manual rather than derived: the wildcard host and the 1086 default port are
    // concrete values a field-wise derive cannot express.
    fn default() -> ManagementConfig {
        ManagementConfig {
            mode: ManagementMode::SeparatePort,
            host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 1086,
            metrics: MetricsExposure::Disabled,
        }
    }
}

impl ManagementConfig {
    /// Validate the management-plane configuration in isolation.
    ///
    /// The cross-section rule that the separate management port must differ from
    /// the data-plane port cannot be checked here, since it spans two sections;
    /// it is enforced by the aggregate validation pipeline.
    ///
    /// # Errors
    ///
    /// Returns [`ManagementConfigError::ZeroPort`] when a `separate-port` plane is
    /// configured on port zero, and [`ManagementConfigError::MetricsWithoutManagementPlane`]
    /// when metrics are requested with no management plane to serve them.
    pub const fn validate(&self) -> Result<(), ManagementConfigError> {
        match self.mode {
            ManagementMode::SeparatePort => {
                if self.port == 0 {
                    return Err(ManagementConfigError::ZeroPort);
                }
            }
            ManagementMode::Merged | ManagementMode::Disabled => {}
        }
        match (self.mode, self.metrics) {
            (ManagementMode::Disabled, MetricsExposure::Prometheus) => Err(ManagementConfigError::MetricsWithoutManagementPlane),
            (ManagementMode::SeparatePort | ManagementMode::Merged | ManagementMode::Disabled, MetricsExposure::Prometheus | MetricsExposure::Disabled) => {
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::management::{ManagementConfig, ManagementConfigError, ManagementMode, MetricsExposure};
    use std::str::FromStr;

    #[test]
    fn mode_defaults_to_separate_port() {
        pretty_assertions::assert_eq!(ManagementConfig::default().mode, ManagementMode::SeparatePort);
    }

    #[test]
    fn port_defaults_to_1086() {
        pretty_assertions::assert_eq!(ManagementConfig::default().port, 1086);
    }

    #[test]
    fn metrics_defaults_to_disabled() {
        pretty_assertions::assert_eq!(ManagementConfig::default().metrics, MetricsExposure::Disabled);
    }

    #[test]
    fn mode_and_metrics_round_trip_kebab_case_through_serde_and_from_str() {
        assert_eq!(serde_json::to_string(&ManagementMode::SeparatePort).expect("serialises"), "\"separate-port\"");
        assert_eq!(serde_json::to_string(&ManagementMode::Merged).expect("serialises"), "\"merged\"");
        assert_eq!(serde_json::to_string(&ManagementMode::Disabled).expect("serialises"), "\"disabled\"");
        assert_eq!(serde_json::to_string(&MetricsExposure::Prometheus).expect("serialises"), "\"prometheus\"");
        assert_eq!(ManagementMode::from_str("separate-port").expect("parses"), ManagementMode::SeparatePort);
        assert_eq!(ManagementMode::from_str("merged").expect("parses"), ManagementMode::Merged);
        assert_eq!(ManagementMode::from_str("disabled").expect("parses"), ManagementMode::Disabled);
        assert_eq!(MetricsExposure::from_str("prometheus").expect("parses"), MetricsExposure::Prometheus);
    }

    #[test]
    fn separate_port_zero_is_rejected() {
        let config = ManagementConfig {
            mode: ManagementMode::SeparatePort,
            port: 0,
            ..ManagementConfig::default()
        };
        pretty_assertions::assert_eq!(config.validate(), Err(ManagementConfigError::ZeroPort));
    }

    #[test]
    fn disabled_plane_with_prometheus_metrics_is_rejected() {
        let config = ManagementConfig {
            mode: ManagementMode::Disabled,
            metrics: MetricsExposure::Prometheus,
            ..ManagementConfig::default()
        };
        pretty_assertions::assert_eq!(config.validate(), Err(ManagementConfigError::MetricsWithoutManagementPlane));
    }

    #[test]
    fn merged_plane_with_prometheus_metrics_is_accepted() {
        let config = ManagementConfig {
            mode: ManagementMode::Merged,
            metrics: MetricsExposure::Prometheus,
            ..ManagementConfig::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn disabled_mode_ignores_port_zero() {
        // A disabled plane binds nothing, so its port is never read and a zero is
        // not an error.
        let config = ManagementConfig {
            mode: ManagementMode::Disabled,
            port: 0,
            metrics: MetricsExposure::Disabled,
            ..ManagementConfig::default()
        };
        assert!(config.validate().is_ok());
    }
}
