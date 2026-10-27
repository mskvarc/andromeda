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

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::{
        listener_transport::ListenerTransport,
        management::{ManagementConfig, ManagementMode},
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
}
