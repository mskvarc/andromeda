//! How the connections a listener accepted come to an end.

use andromeda_configuration::schema::server::ServerConfig;
use std::time::Duration;

/// How the connections a listener accepted come to an end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClosePolicy {
    /// How long connections still in flight when shutdown begins may take to
    /// finish before serving fails with a drain deadline.
    pub drain_deadline: Duration,
    /// How long a connection stays half-closed, unread, after a response on
    /// it announced `Connection: close`, before it is closed.
    ///
    /// RFC 9112 clause 9.6: closing at once while the client is still sending
    /// makes the server's TCP stack reset the connection, and the reset "might
    /// erase the client's unacknowledged input buffers before they can be read".
    /// The delay gives the client time to read the response first.
    pub announced_close_delay: Duration,
}

impl From<&ServerConfig> for ClosePolicy {
    fn from(server: &ServerConfig) -> ClosePolicy {
        ClosePolicy {
            drain_deadline: server.graceful_drain_timeout.duration(),
            announced_close_delay: server.announced_close_timeout.duration(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::close_policy::ClosePolicy;
    use andromeda_configuration::schema::server::ServerConfig;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn the_default_policy_delays_an_announced_close_by_one_second_and_drains_for_thirty() {
        assert_eq!(
            ClosePolicy::from(&ServerConfig::default()),
            ClosePolicy {
                drain_deadline: Duration::from_secs(30),
                announced_close_delay: Duration::from_secs(1),
            }
        );
    }
}
