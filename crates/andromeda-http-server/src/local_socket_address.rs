//! Where a broker's local socket lives, and connecting to it.
//!
//! The socket is named after the API port: `andromeda-<port>.sock` in the
//! configured directory on Unix, the named pipe `\\.\pipe\andromeda-<port>` on
//! Windows. Two brokers on one host already need API ports of their own, so
//! two sockets share a name only where two API listeners would share a port.

use andromeda_configuration::schema::{management::ManagementConfig, server::ServerConfig};
use interprocess::local_socket::{ConnectOptions, GenericFilePath, Name, ToFsName, tokio::Stream};
use std::{
    io,
    path::{Path, PathBuf},
};

/// The stem every local socket name starts with.
const NAME_STEM: &str = "andromeda";

/// The address of a broker's local socket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalSocketAddress {
    /// The socket: a file on Unix, a named pipe on Windows.
    path: PathBuf,
}

impl LocalSocketAddress {
    /// The socket of the broker whose API listens on `port`, kept in
    /// `directory`.
    #[must_use]
    pub fn new(directory: &Path, port: u16) -> LocalSocketAddress {
        LocalSocketAddress {
            path: socket_path(directory, port),
        }
    }

    /// The socket of the broker that `server` and `management` configure.
    #[must_use]
    pub fn configured(server: &ServerConfig, management: &ManagementConfig) -> LocalSocketAddress {
        LocalSocketAddress::new(&management.resolved_local_socket_directory(), server.port)
    }

    /// The socket's path: the socket file on Unix, the pipe on Windows.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The socket's name as the local socket transport addresses it.
    ///
    /// # Errors
    ///
    /// Returns the transport's error when the platform cannot address the
    /// path as a local socket.
    pub(crate) fn name(&self) -> io::Result<Name<'_>> {
        self.path.as_path().to_fs_name::<GenericFilePath>()
    }

    /// Open a connection to the broker listening on the socket.
    ///
    /// # Errors
    ///
    /// Returns the error of the connection attempt: `NotFound` when no socket
    /// exists at the path, `ConnectionRefused` when no broker listens on it any
    /// more, `PermissionDenied` when the calling user may not open it.
    pub async fn connect(&self) -> io::Result<Stream> {
        ConnectOptions::new().name(self.name()?).connect_tokio().await
    }
}

/// The socket file in `directory` of the broker listening on `port`.
#[cfg(unix)]
fn socket_path(directory: &Path, port: u16) -> PathBuf {
    directory.join(format!("{NAME_STEM}-{port}.sock"))
}

/// The named pipe of the broker listening on `port`; pipes live in a
/// namespace of their own, so `directory` plays no part.
#[cfg(windows)]
fn socket_path(_directory: &Path, port: u16) -> PathBuf {
    PathBuf::from(format!(r"\\.\pipe\{NAME_STEM}-{port}"))
}

#[cfg(test)]
mod tests {
    use crate::local_socket_address::LocalSocketAddress;
    use andromeda_configuration::schema::{
        management::{ManagementConfig, ManagementMode},
        server::ServerConfig,
    };
    use pretty_assertions::assert_eq;
    use std::path::{Path, PathBuf};

    #[cfg(unix)]
    #[test]
    fn the_socket_is_named_after_the_api_port_in_its_directory() {
        let address = LocalSocketAddress::new(Path::new("/run/andromeda"), 1056);
        assert_eq!(address.path(), Path::new("/run/andromeda/andromeda-1056.sock"));
    }

    #[cfg(windows)]
    #[test]
    fn the_socket_is_a_named_pipe_named_after_the_api_port() {
        let address = LocalSocketAddress::new(Path::new(r"C:\ProgramData\andromeda"), 1056);
        assert_eq!(address.path(), Path::new(r"\\.\pipe\andromeda-1056"));
    }

    #[test]
    fn the_configured_socket_takes_the_configured_directory_and_the_api_port_in_every_management_mode() {
        let server = ServerConfig {
            port: 2000,
            ..ServerConfig::default()
        };
        for mode in [ManagementMode::SeparatePort, ManagementMode::SharedPort, ManagementMode::Disabled] {
            let management = ManagementConfig {
                mode,
                local_socket_directory: Some(PathBuf::from("/srv/andromeda/run")),
                ..ManagementConfig::default()
            };
            assert_eq!(
                LocalSocketAddress::configured(&server, &management),
                LocalSocketAddress::new(Path::new("/srv/andromeda/run"), 2000)
            );
        }
    }
}
