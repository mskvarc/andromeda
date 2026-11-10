//! The address the brokers reach this process by: the one the stack sets, or else the one read off the route to the
//! front broker.
//!
//! Connecting a UDP socket sends nothing: it only selects the route, and the socket's local address is then this
//! container's address on the network the front broker is on.

use crate::{
    config::{BrokerRole, SuiteConfig},
    config_error::ConfigError,
};
use std::{
    net::{IpAddr, UdpSocket},
    sync::{Arc, OnceLock},
};
use url::{Host, Url};

/// The callback host found from the route to the front broker, once per test process.
static DISCOVERED_CALLBACK_HOST: OnceLock<Result<Host<String>, ConfigError>> = OnceLock::new();

/// The host the brokers reach the test process by.
///
/// A host the stack sets wins. Otherwise it is the local address of the route to the front broker, which is the
/// address of this container on the stack network: each runner container has its own, so two runs against one stack
/// never receive each other's callbacks.
///
/// # Errors
///
/// Returns [`ConfigError`] when no front broker is configured or no route to it exists.
pub(crate) fn callback_host(config: &SuiteConfig) -> Result<Host<String>, ConfigError> {
    match config.configured_callback_host() {
        Some(host) => Ok(host.clone()),
        None => DISCOVERED_CALLBACK_HOST
            .get_or_init(|| config.broker(BrokerRole::Front).and_then(local_host_towards))
            .clone(),
    }
}

/// The local address of the route to `front`.
///
/// # Errors
///
/// Returns [`ConfigError::FrontWithoutHost`] when `front` names no host, [`ConfigError::RouteSocket`] when no socket
/// can be opened, and [`ConfigError::NoRoute`] when no route leads to it.
fn local_host_towards(front: &Url) -> Result<Host<String>, ConfigError> {
    // clone: the error must own the front broker URL it reports
    let host = front.host_str().ok_or_else(|| ConfigError::FrontWithoutHost(front.clone()))?;
    let port = front.port_or_known_default().unwrap_or(80);
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|error| ConfigError::RouteSocket(Arc::new(error)))?;
    socket.connect((host, port)).map_err(|error| ConfigError::NoRoute {
        // clone: the error must own the front broker URL it reports
        front: front.clone(),
        source: Arc::new(error),
    })?;
    let local = socket.local_addr().map_err(|error| ConfigError::RouteSocket(Arc::new(error)))?;
    Ok(match local.ip() {
        IpAddr::V4(address) => Host::Ipv4(address),
        IpAddr::V6(address) => Host::Ipv6(address),
    })
}
