//! The listener of a broker's local socket.
//!
//! On Unix the socket is a file in a directory the broker creates so that only
//! its own user can open it, and on Linux the socket file is owner-only too.
//! BSD-derived systems, macOS among them, ignore a socket file's mode on
//! connect, so the directory is what keeps other users out there. Beside the
//! socket the broker holds an exclusive lock on `<socket>.lock` for as long as
//! it listens. The kernel releases the lock when the process ends, however it
//! ends, so a socket file under a free lock was left by a broker that stopped
//! and is replaced, and one under a held lock belongs to a running broker and
//! is left alone. The lock file is never removed: unlinking it would let a
//! later broker lock a fresh file while another still holds the unlinked one.
//!
//! On Windows the socket is a named pipe, which disappears with its process
//! and which no second process can create while it exists.

use crate::{accept_backoff::back_off_after, local_socket_address::LocalSocketAddress};
use axum::serve::Listener;
use interprocess::local_socket::{
    ListenerOptions,
    tokio::{Listener as LocalListener, Stream},
    traits::tokio::Listener as _,
};
#[cfg(target_os = "linux")]
use interprocess::os::unix::local_socket::ListenerOptionsExt;
#[cfg(unix)]
use std::{
    fs::{DirBuilder, File, OpenOptions, TryLockError},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
};
use std::{
    io,
    path::{Path, PathBuf},
};
use thiserror::Error;

/// Why a local socket could not be served.
#[derive(Debug, Error)]
pub enum LocalSocketError {
    /// A running broker already listens on the socket.
    #[error("another running broker listens on the local socket {}", .socket.display())]
    InUse {
        /// The socket.
        socket: PathBuf,
    },
    /// The directory holding the socket could not be created.
    #[error("the local socket directory {} could not be created: {source}", .directory.display())]
    Directory {
        /// The directory.
        directory: PathBuf,
        /// The filesystem error.
        source: io::Error,
    },
    /// The lock beside the socket could not be opened or taken.
    #[error("the lock {} of the local socket could not be taken: {source}", .lock.display())]
    Lock {
        /// The lock file.
        lock: PathBuf,
        /// The filesystem error.
        source: io::Error,
    },
    /// The socket could not be bound.
    #[error("the local socket {} could not be bound: {source}", .socket.display())]
    Bind {
        /// The socket.
        socket: PathBuf,
        /// The operating-system error.
        source: io::Error,
    },
}

/// The peer of a connection on a local socket, which carries no address.
#[derive(Clone, Copy, Debug)]
pub struct LocalSocketPeer;

/// A bound local socket accepting connections.
pub struct LocalSocketListener {
    /// The listening socket. Declared before the lock, so that dropping the
    /// listener removes the socket file while the lock still claims its name.
    listener: LocalListener,
    /// The exclusive lock claiming the socket's name, held and never read:
    /// dropping it after the listener releases the name.
    #[cfg(unix)]
    _lock: File,
    /// The socket's path.
    path: PathBuf,
}

impl LocalSocketListener {
    /// Bind the socket at `address`, claiming its name first on Unix.
    ///
    /// Runs inside a Tokio runtime.
    ///
    /// # Errors
    ///
    /// Returns [`LocalSocketError::InUse`] when a running broker listens on
    /// the socket, [`LocalSocketError::Directory`] when its directory cannot be
    /// created, [`LocalSocketError::Lock`] when the lock beside it cannot be
    /// opened or taken, and [`LocalSocketError::Bind`] when the socket cannot
    /// be bound.
    pub fn bind(address: &LocalSocketAddress) -> Result<LocalSocketListener, LocalSocketError> {
        let socket = address.path();
        #[cfg(unix)]
        let lock = claim(socket)?;
        let bind_error = |source| LocalSocketError::Bind {
            socket: socket.to_path_buf(),
            source,
        };
        // Under the claimed name an existing socket file is a stopped broker's,
        // so binding over it is safe.
        let options = ListenerOptions::new().name(address.name().map_err(bind_error)?).try_overwrite(true);
        #[cfg(target_os = "linux")]
        let options = options.mode(0o600);
        let listener = options.create_tokio().map_err(|source| {
            if source.kind() == io::ErrorKind::AddrInUse {
                LocalSocketError::InUse { socket: socket.to_path_buf() }
            } else {
                bind_error(source)
            }
        })?;
        Ok(LocalSocketListener {
            listener,
            #[cfg(unix)]
            _lock: lock,
            path: socket.to_path_buf(),
        })
    }

    /// The socket's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Listener for LocalSocketListener {
    type Io = Stream;
    type Addr = LocalSocketPeer;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match self.listener.accept().await {
                Ok(stream) => return (stream, LocalSocketPeer),
                Err(error) => back_off_after(error).await,
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(LocalSocketPeer)
    }
}

/// Create the directory of `socket` if it is missing, and take the exclusive
/// lock beside the socket.
///
/// # Errors
///
/// Returns [`LocalSocketError::Directory`] when the directory cannot be
/// created, [`LocalSocketError::InUse`] when another process holds the lock,
/// and [`LocalSocketError::Lock`] when the lock cannot be opened or taken.
#[cfg(unix)]
fn claim(socket: &Path) -> Result<File, LocalSocketError> {
    if let Some(directory) = socket.parent() {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)
            .map_err(|source| LocalSocketError::Directory {
                directory: directory.to_path_buf(),
                source,
            })?;
    }
    let lock_path = socket.with_added_extension("lock");
    let lock = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .open(&lock_path)
        .map_err(|source| LocalSocketError::Lock {
            lock: lock_path.clone(), // clone: the path also names the lock when `try_lock` fails.
            source,
        })?;
    match lock.try_lock() {
        Ok(()) => Ok(lock),
        Err(TryLockError::WouldBlock) => Err(LocalSocketError::InUse { socket: socket.to_path_buf() }),
        Err(TryLockError::Error(source)) => Err(LocalSocketError::Lock { lock: lock_path, source }),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use crate::{
        local_socket_address::LocalSocketAddress,
        local_socket_listener::{LocalSocketError, LocalSocketListener},
    };
    use axum::serve::Listener;
    use pretty_assertions::assert_eq;
    use std::{
        error::Error,
        fs,
        os::unix::{fs::PermissionsExt, net::UnixListener},
    };
    use tempfile::TempDir;

    #[tokio::test]
    async fn binding_creates_a_directory_only_its_user_can_open_and_a_socket_that_accepts_connections() -> Result<(), Box<dyn Error>> {
        let root = TempDir::new()?;
        let directory = root.path().join("run").join("andromeda");
        let address = LocalSocketAddress::new(&directory, 1056);
        let mut listener = LocalSocketListener::bind(&address)?;
        assert_eq!(fs::metadata(&directory)?.permissions().mode() & 0o777, 0o700);
        let accepted = tokio::spawn(async move { listener.accept().await });
        address.connect().await?;
        accepted.await?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn on_linux_the_socket_file_is_owner_only() -> Result<(), Box<dyn Error>> {
        let root = TempDir::new()?;
        let address = LocalSocketAddress::new(root.path(), 1056);
        let _listener = LocalSocketListener::bind(&address)?;
        assert_eq!(fs::metadata(address.path())?.permissions().mode() & 0o777, 0o600);
        Ok(())
    }

    #[tokio::test]
    async fn a_socket_a_running_listener_holds_is_refused_as_in_use() -> Result<(), Box<dyn Error>> {
        let root = TempDir::new()?;
        let address = LocalSocketAddress::new(root.path(), 1056);
        let _first = LocalSocketListener::bind(&address)?;
        assert!(matches!(LocalSocketListener::bind(&address), Err(LocalSocketError::InUse { .. })));
        Ok(())
    }

    #[tokio::test]
    async fn a_socket_file_a_stopped_broker_left_behind_is_replaced() -> Result<(), Box<dyn Error>> {
        let root = TempDir::new()?;
        let address = LocalSocketAddress::new(root.path(), 1056);
        drop(UnixListener::bind(address.path())?);
        assert!(address.path().exists());
        let mut listener = LocalSocketListener::bind(&address)?;
        let accepted = tokio::spawn(async move { listener.accept().await });
        address.connect().await?;
        accepted.await?;
        Ok(())
    }

    #[tokio::test]
    async fn dropping_the_listener_removes_the_socket_keeps_the_lock_file_and_frees_the_name() -> Result<(), Box<dyn Error>> {
        let root = TempDir::new()?;
        let address = LocalSocketAddress::new(root.path(), 1056);
        drop(LocalSocketListener::bind(&address)?);
        assert!(!address.path().exists());
        assert!(address.path().with_added_extension("lock").exists());
        LocalSocketListener::bind(&address)?;
        Ok(())
    }

    #[tokio::test]
    async fn a_directory_that_cannot_be_created_is_reported_with_its_path() -> Result<(), Box<dyn Error>> {
        let root = TempDir::new()?;
        let file = root.path().join("file");
        fs::write(&file, b"")?;
        let directory = file.join("run");
        let outcome = LocalSocketListener::bind(&LocalSocketAddress::new(&directory, 1056));
        assert!(matches!(outcome, Err(LocalSocketError::Directory { directory: reported, .. }) if reported == directory));
        Ok(())
    }
}
