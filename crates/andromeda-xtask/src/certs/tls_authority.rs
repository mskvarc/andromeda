//! The certificate material of the TLS stack.
//!
//! One authority issues the server leaves every TLS-terminating service of the
//! stack presents -- the nginx terminator and the native-TLS brokers alike --
//! so the suite trusts that one authority. A second authority issues the client
//! certificate the mutual-TLS brokers accept, and a self-signed client
//! certificate stands in for one from an issuer they do not know. The set is
//! committed test material and is generated only when a file of it is missing.
//!
//! The rotating broker reads its own copy of the first leaf from a directory the
//! suite rewrites. A run that stopped mid-rotation leaves the other leaf there,
//! so that copy is reset from the first leaf before every run.

use crate::{
    certs::openssl::{Signer, issue},
    error::XtaskError,
};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{fs, path::Path};

/// Directory that holds the generated material, relative to the repository root.
const CERTS_DIR: &str = "docker/compose/test/tls/certs";
/// Subdirectory the rotating broker reads its identity from, and the suite rewrites.
const ROTATING_DIR: &str = "rotating";
/// Extensions of a server leaf valid for the loopback names the suite dials.
const SERVER_LEAF: [&str; 3] = [
    "subjectAltName=DNS:localhost,IP:127.0.0.1",
    "basicConstraints=critical,CA:FALSE",
    "extendedKeyUsage=serverAuth",
];
/// Extensions of a client leaf.
const CLIENT_LEAF: [&str; 2] = ["basicConstraints=critical,CA:FALSE", "extendedKeyUsage=clientAuth"];
/// The files of the set, each of which must exist for it to be reused.
const MATERIAL: [&str; 12] = [
    "ca.crt",
    "ca.key",
    "broker.crt",
    "broker.key",
    "rotated.crt",
    "rotated.key",
    "client-ca.crt",
    "client-ca.key",
    "client.crt",
    "client.key",
    "stranger.crt",
    "stranger.key",
];
/// Extensions of an authority.
const AUTHORITY: [&str; 2] = ["basicConstraints=critical,CA:TRUE", "keyUsage=critical,keyCertSign,cRLSign"];

/// Generate the TLS stack's material if any of it is missing, then reset the
/// rotating broker's copy of its identity.
///
/// # Errors
///
/// Returns [`XtaskError::Io`] when a directory or file cannot be written and
/// [`XtaskError::CertGeneration`] when `openssl` exits non-zero.
pub(super) fn ensure(repository_root: &Path) -> Result<(), XtaskError> {
    let directory = repository_root.join(CERTS_DIR);
    fs::create_dir_all(directory.join(ROTATING_DIR))?;
    if !MATERIAL.iter().all(|file| directory.join(file).exists()) {
        generate(&directory)?;
    }
    for (source, rotating) in [("broker.crt", "tls.crt"), ("broker.key", "tls.key")] {
        fs::copy(directory.join(source), directory.join(ROTATING_DIR).join(rotating))?;
    }
    readable_by_the_broker_containers(&directory)?;
    readable_by_the_broker_containers(&directory.join(ROTATING_DIR))
}

/// Generate the whole set: every leaf depends on its authority, so no file of
/// it is regenerated alone.
fn generate(directory: &Path) -> Result<(), XtaskError> {
    issue(directory, "/CN=Andromeda TLS stack CA", &AUTHORITY, &Signer::SelfSigned, "ca.crt", "ca.key")?;
    let server_authority = Signer::Authority {
        certificate: Path::new("ca.crt"),
        private_key: Path::new("ca.key"),
    };
    issue(directory, "/CN=localhost", &SERVER_LEAF, &server_authority, "broker.crt", "broker.key")?;
    issue(directory, "/CN=localhost", &SERVER_LEAF, &server_authority, "rotated.crt", "rotated.key")?;
    issue(
        directory,
        "/CN=Andromeda TLS stack client CA",
        &AUTHORITY,
        &Signer::SelfSigned,
        "client-ca.crt",
        "client-ca.key",
    )?;
    let client_authority = Signer::Authority {
        certificate: Path::new("client-ca.crt"),
        private_key: Path::new("client-ca.key"),
    };
    issue(
        directory,
        "/CN=Andromeda TLS stack client",
        &CLIENT_LEAF,
        &client_authority,
        "client.crt",
        "client.key",
    )?;
    issue(
        directory,
        "/CN=Andromeda TLS stack stranger",
        &CLIENT_LEAF,
        &Signer::SelfSigned,
        "stranger.crt",
        "stranger.key",
    )
}

/// Make every generated file world-readable.
///
/// `openssl` writes private keys owner-only, but the broker containers run as
/// their own unprivileged user and read the files through a bind mount, where a
/// Linux host enforces the host file's mode. The keys are throwaway test
/// material, so readability is the only property that matters.
#[cfg(unix)]
fn readable_by_the_broker_containers(directory: &Path) -> Result<(), XtaskError> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_file() {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
        }
    }
    Ok(())
}

/// Windows file modes do not gate a Docker bind mount; nothing to relax.
#[cfg(not(unix))]
fn readable_by_the_broker_containers(_directory: &Path) -> Result<(), XtaskError> {
    Ok(())
}
