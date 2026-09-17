//! The host-broker dev loop for the base tier: an incremental cargo build and two
//! spawned broker processes, in place of the containerized `broker` profile.

use crate::error::XtaskError;
use reqwest::blocking::Client;
use std::{
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    thread::sleep,
    time::Duration,
};

/// Port of the broker under test.
pub(crate) const BROKER_PORT: u16 = 1056;
/// Port of the trusted-edge broker.
pub(crate) const TRUSTED_PORT: u16 = 1057;
/// Database endpoint both host brokers share.
const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";
/// The tier pins `default_last_n` to 19 so the `SYSTEM_LAST_N_CAP` regressions hit the 206 cut.
const DEFAULT_LAST_N: &str = "19";
/// Health-poll budget: 120 attempts one second apart.
const READINESS_ATTEMPTS: u16 = 120;

/// Which forwarding-header trust the spawned broker runs with.
#[derive(Clone, Copy)]
pub(crate) enum ForwardedHeaders {
    /// The default: forwarding headers are not trusted.
    Untrusted,
    /// The trusted-edge half: forwarding headers are trusted.
    Trusted,
}

/// Fail fast if either host-broker port is already bound.
///
/// # Errors
///
/// Returns [`XtaskError::PortInUse`] for the first port that cannot be bound.
pub(crate) fn preflight_ports() -> Result<(), XtaskError> {
    for port in [BROKER_PORT, TRUSTED_PORT] {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => drop(listener),
            Err(_bind_error) => return Err(XtaskError::PortInUse(port)),
        }
    }
    Ok(())
}

/// Build the broker binary in release mode for the host-broker loop.
///
/// # Errors
///
/// Returns [`XtaskError::CargoBuild`] on a non-zero exit, and [`XtaskError::Io`] when
/// the process cannot be spawned.
pub(crate) fn build(repository_root: &Path) -> Result<(), XtaskError> {
    let status = Command::new("cargo")
        .args(["build", "--release", "--bin", "andromeda"])
        .current_dir(repository_root)
        .status()?;
    if status.success() { Ok(()) } else { Err(XtaskError::CargoBuild) }
}

/// Spawn one host broker on the given port. The caller owns the returned child and
/// is responsible for tearing it down.
///
/// # Errors
///
/// Returns [`XtaskError::Io`] when the broker process cannot be spawned.
pub(crate) fn spawn_broker(repository_root: &Path, port: u16, forwarded: ForwardedHeaders) -> Result<Child, XtaskError> {
    let binary = repository_root.join("target/release/andromeda");
    let mut command = Command::new(binary);
    command
        .current_dir(repository_root)
        .env("ANDROMEDA__SERVER__HOST", "127.0.0.1")
        .env("ANDROMEDA__SERVER__PORT", port.to_string())
        .env("ANDROMEDA__DATABASE__URL", DATABASE_URL)
        .env("ANDROMEDA__TEMPORAL__DEFAULT_LAST_N", DEFAULT_LAST_N)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match forwarded {
        ForwardedHeaders::Untrusted => {}
        ForwardedHeaders::Trusted => {
            command.env("ANDROMEDA__HTTP__FORWARDED_HEADERS", "trusted");
        }
    }
    let child = command.spawn()?;
    Ok(child)
}

/// Poll both host brokers' health endpoints until they answer or the budget is spent.
///
/// # Errors
///
/// Returns [`XtaskError::HealthClient`] when the HTTP client cannot be built and
/// [`XtaskError::BrokerNotReady`] for the first broker that never answers.
pub(crate) fn await_ready() -> Result<(), XtaskError> {
    let client = Client::builder().timeout(Duration::from_secs(2)).build()?;
    for port in [BROKER_PORT, TRUSTED_PORT] {
        let url = format!("http://127.0.0.1:{port}/ngsi-ld/v1/health");
        let mut ready = false;
        for _attempt in 0..READINESS_ATTEMPTS {
            if client.get(&url).send().is_ok_and(|response| response.status().is_success()) {
                ready = true;
                break;
            }
            sleep(Duration::from_secs(1));
        }
        if !ready {
            return Err(XtaskError::BrokerNotReady(port));
        }
    }
    Ok(())
}
