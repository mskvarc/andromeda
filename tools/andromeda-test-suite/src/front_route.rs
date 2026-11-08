//! The address the brokers reach this process by, read off the route to the front broker.
//!
//! Connecting a UDP socket sends nothing: it only selects the route, and the socket's local address is then this
//! container's address on the network the front broker is on.

use crate::config_error::ConfigError;
use std::{
    net::{IpAddr, UdpSocket},
    sync::Arc,
};
use url::{Host, Url};

/// The local address of the route to `front`.
///
/// # Errors
///
/// Returns [`ConfigError::FrontWithoutHost`] when `front` names no host, [`ConfigError::RouteSocket`] when no socket
/// can be opened, and [`ConfigError::NoRoute`] when no route leads to it.
pub(crate) fn local_host_towards(front: &Url) -> Result<Host<String>, ConfigError> {
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
