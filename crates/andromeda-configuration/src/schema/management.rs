//! The `[management]` table: where the broker serves its operational
//! endpoints.
//!
//! The management plane carries `/livez`, `/readyz` and, when
//! `[observability.metrics]` enables them, `/metrics` on a bare middleware
//! stack, isolated from the public NGSI-LD data plane so a Prometheus scraper
//! or a kubelet is never subject to CORS, CSRF, security headers, body limits
//! or compression, and so probes and metrics keep answering while the data
//! plane is saturated or draining.
//!
//! Beside whatever network listener the mode chooses, the broker serves
//! `/livez` and `/readyz` on a local socket in every mode, the channel the
//! `health` command asks. It is reachable only from the same host, or the same
//! container, as the user the broker runs as.

use crate::{platform_directories::default_runtime_directory, schema::listener_transport::ListenerTransport};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
};
use strum::{Display, EnumString};

/// Where, and whether, the broker serves its management endpoints.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "Whether, and where, the broker serves its management endpoints on the network: `/livez`, `/readyz` and `/metrics`.")]
pub enum ManagementMode {
    /// No management listener on the network: no network client reaches a
    /// probe, and metrics cannot be enabled. The local socket still answers
    /// the `health` command.
    #[schemars(
        description = "Serves no management endpoints on the network, so no network client reaches a liveness or readiness probe and metrics cannot be enabled. The local socket still answers the `health` command."
    )]
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

/// The management plane's topology, its listener and its local socket.
///
/// `host`, `port` and `transport` are read only in [`ManagementMode::SeparatePort`]; in
/// [`ManagementMode::SharedPort`] the endpoints ride the data-plane listener, and in
/// [`ManagementMode::Disabled`] no management listener is bound. The local
/// socket in `local_socket_directory` is served in every mode.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone)]
#[schemars(deny_unknown_fields)]
#[schemars(
    description = "Whether, and where, the broker serves its management endpoints: `/livez`, `/readyz` and `/metrics` on the network, and the probes on a local socket for the `health` command."
)]
pub struct ManagementConfig {
    /// Where the management endpoints are served: `disabled`, `shared-port` or
    /// `separate-port`.
    #[schemars(
        description = "Where the management endpoints are served: on a port of their own, on the API port, or not at all. A port of their own by default, so probes and metrics keep answering while the API is saturated or draining."
    )]
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
    /// The directory holding the local socket the `health` command asks.
    /// `None` takes the platform default that [`default_runtime_directory`]
    /// resolves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        title = "path",
        description = "The directory holding the local socket that answers the `health` command, served in every mode and named after `server.port`, such as `andromeda-1056.sock`. Left out, a Linux service outside a user session, a container among them, uses `/run/andromeda`; a user session uses `andromeda` in `$XDG_RUNTIME_DIR`, and macOS without one the per-user temporary directory. The broker creates it so that only its own user can open it. A relative path is read from the working directory, of the broker and of the `health` command alike. On Windows the socket is the named pipe `\\\\.\\pipe\\andromeda-<port>` and the directory is not read."
    )]
    pub local_socket_directory: Option<PathBuf>,
}

impl ManagementConfig {
    /// The directory holding the local socket: the configured one, or the
    /// platform default.
    #[must_use]
    pub fn resolved_local_socket_directory(&self) -> Cow<'_, Path> {
        self.local_socket_directory
            .as_deref()
            .map_or_else(|| Cow::Owned(default_runtime_directory()), Cow::Borrowed)
    }
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
            local_socket_directory: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        platform_directories::default_runtime_directory,
        schema::{
            listener_transport::ListenerTransport,
            management::{ManagementConfig, ManagementMode},
        },
    };
    use std::{
        path::{Path, PathBuf},
        str::FromStr,
    };

    #[test]
    fn a_configured_local_socket_directory_is_used_verbatim() {
        let config = ManagementConfig {
            local_socket_directory: Some(PathBuf::from("/srv/andromeda/run")),
            ..ManagementConfig::default()
        };
        pretty_assertions::assert_eq!(config.resolved_local_socket_directory(), Path::new("/srv/andromeda/run"));
    }

    #[test]
    fn a_local_socket_directory_left_out_resolves_to_the_platform_runtime_directory() {
        pretty_assertions::assert_eq!(ManagementConfig::default().resolved_local_socket_directory(), default_runtime_directory());
    }

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
