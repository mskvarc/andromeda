//! How the connections a listener accepted come to an end.

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
