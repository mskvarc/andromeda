//! Whether the broker serves, or refuses to start, when its local socket
//! cannot be bound.

use crate::listeners::{ListenerError, bind_local_socket};
use andromeda_http_server::{local_socket_address::LocalSocketAddress, local_socket_listener::LocalSocketListener};
use std::error::Error;
#[cfg(unix)]
use std::fs;
use tempfile::TempDir;

#[tokio::test]
async fn a_free_local_socket_is_bound() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    assert!(bind_local_socket(&LocalSocketAddress::new(directory.path(), 1056))?.is_some());
    Ok(())
}

#[tokio::test]
async fn a_local_socket_another_running_broker_holds_refuses_startup() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let address = LocalSocketAddress::new(directory.path(), 1056);
    let _held = LocalSocketListener::bind(&address)?;
    assert!(matches!(bind_local_socket(&address), Err(ListenerError::LocalSocketClaim(_))));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn a_socket_directory_the_broker_cannot_create_leaves_it_serving_without_the_socket() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    let file = directory.path().join("file");
    fs::write(&file, b"")?;
    assert!(bind_local_socket(&LocalSocketAddress::new(&file.join("run"), 1056))?.is_none());
    Ok(())
}
