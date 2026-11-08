//! How long an accepted TCP connection may go without carrying a request.

use andromeda_configuration::schema::server::ServerConfig;
use std::time::Duration;

/// How long an accepted TCP connection may go without carrying a request
/// before it is closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectionBounds {
    /// How long a request head may take to arrive: from the connection's
    /// opening until its first request, and over HTTP/1.1 from the first octet
    /// of each later head until the head is complete.
    pub head: Duration,
    /// How long the connection may stay open with no request in flight and no
    /// request head arriving.
    pub idle: Duration,
}

impl From<&ServerConfig> for ConnectionBounds {
    fn from(server: &ServerConfig) -> ConnectionBounds {
        ConnectionBounds {
            head: server.request_head_timeout.duration(),
            idle: server.idle_timeout.duration(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::connection_bounds::ConnectionBounds;
    use andromeda_configuration::schema::server::ServerConfig;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn the_default_bounds_wait_thirty_seconds_for_a_head_and_three_minutes_while_idle() {
        assert_eq!(
            ConnectionBounds::from(&ServerConfig::default()),
            ConnectionBounds {
                head: Duration::from_secs(30),
                idle: Duration::from_secs(180),
            }
        );
    }
}
