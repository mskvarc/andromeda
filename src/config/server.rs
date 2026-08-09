//! HTTP listener configuration.

use confique::Config;
use serde::Serialize;
use std::net::IpAddr;

/// Command-line listener overrides applied above every other configuration layer.
#[derive(Debug, Clone, Copy, Default)]
pub struct ServerOverrides {
    /// Optional listener address override.
    pub host: Option<IpAddr>,
    /// Optional listener port override.
    pub port: Option<u16>,
}

/// Address on which the broker accepts HTTP traffic.
#[derive(Config, Debug, Clone, Serialize)]
pub struct ServerConfig {
    /// Listener address.
    #[config(env = "ANDROMEDA_SERVER_HOST", default = "0.0.0.0")]
    pub host: IpAddr,
    /// Listener port.
    #[config(env = "ANDROMEDA_SERVER_PORT", default = 1056)]
    pub port: u16,
}
