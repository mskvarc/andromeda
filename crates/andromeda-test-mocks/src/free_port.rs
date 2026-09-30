//! A loopback TCP port for a process a test starts itself.
//!
//! A test that starts a broker process of its own has to give it a port to
//! listen on before the process exists. The operating system hands out a free
//! ephemeral port to a listener bound to port 0; that listener is dropped at
//! once, leaving the port free for the process the test starts next.

use std::{io::Error as IoError, net::TcpListener};

/// A loopback TCP port nothing listens on right now.
///
/// # Errors
///
/// Returns the I/O error of binding the probe listener or reading its address.
pub fn free_loopback_port() -> Result<u16, IoError> {
    Ok(TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port())
}

#[cfg(test)]
mod tests {
    use crate::free_port::free_loopback_port;
    use std::net::TcpListener;

    #[test]
    fn a_free_loopback_port_can_be_bound_again() {
        let port = free_loopback_port().expect("a free port");

        assert_ne!(port, 0);
        assert!(TcpListener::bind(("127.0.0.1", port)).is_ok(), "the reported port is free");
    }
}
