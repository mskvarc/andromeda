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
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use strum::{Display, EnumString};

/// Where, and whether, the broker serves its management endpoints.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "Where, and whether, the broker serves its management endpoints (`/livez`, `/readyz` and `/metrics`).")]
pub enum ManagementMode {
    /// No management plane is served; the data-plane probes remain and metrics
    /// cannot be enabled.
    #[schemars(description = "Serves no management endpoints. The health paths under `/ngsi-ld/v1` on the API port remain.")]
    Disabled,
    /// The management endpoints are mounted at the root of the data-plane port,
    /// behind the full inbound stack.
    #[schemars(
        description = "Serves the management endpoints at the root of the API port, behind the API's full web stack (CORS, CSRF, security headers, body limits, compression)."
    )]
    SharedPort,
    /// The management endpoints bind their own TCP port, isolated from the public
    /// data plane by a bare middleware stack.
    #[default]
    #[schemars(
        description = "Serves the management endpoints on a listener of their own, `management.host` and `management.port`, on a bare stack, so probes and metrics keep answering while the API is saturated or draining."
    )]
    SeparatePort,
}

/// The management plane's topology and listener.
///
/// `host`, `port` and `transport` are read only in [`ManagementMode::SeparatePort`]; in
/// [`ManagementMode::SharedPort`] the endpoints ride the data-plane listener, and in
/// [`ManagementMode::Disabled`] no management listener is bound.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone)]
#[serde(deny_unknown_fields)]
#[schemars(description = "Where the broker serves its health probes and metrics, kept apart from the NGSI-LD API.")]
pub struct ManagementConfig {
    /// Where the management endpoints are served: `disabled`, `shared-port` or
    /// `separate-port`.
    #[schemars(description = "Where the management endpoints are served: on a port of their own, on the API port, or not at all.")]
    pub mode: ManagementMode,
    /// Management listener address. Read only when `mode` is `separate-port`.
    #[schemars(description = "The address the management listener binds. Read only when `management.mode` is `\"separate-port\"`.")]
    pub host: IpAddr,
    /// Management listener port. Read only when `mode` is `separate-port`.
    #[schemars(description = "The TCP port the management listener binds. Read only when `management.mode` is `\"separate-port\"`.")]
    pub port: u16,
    /// Whether the management listener serves plaintext or terminates TLS with
    /// the `[tls]` identity: `plaintext` or `tls`. Read only when `mode` is
    /// `separate-port`.
    #[schemars(
        description = "Whether the management listener serves plaintext or terminates TLS with the `[tls]` certificate. Read only when `management.mode` is `\"separate-port\"`."
    )]
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
