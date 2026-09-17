//! The broker under test for an ETSI run: built in release mode and spawned as a
//! single host process on the run's port, against the dev compose database, with
//! the temporal page size the suite's pagination precondition requires.

use crate::{error::XtaskError, local_broker, readiness};
use reqwest::blocking::Client;
use std::{
    fs::File,
    path::Path,
    process::{Child, Command, Stdio},
    thread::sleep,
    time::Duration,
};

/// Database endpoint served by `docker/compose/dev`; matches the compose port map.
const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";
/// The suite pages temporal instances at twenty per attribute, so the broker's
/// default last-n is pinned just under that for the duration of the run.
const DEFAULT_LAST_N: &str = "19";
/// Health-poll budget: 120 attempts one second apart.
const READINESS_ATTEMPTS: u16 = 120;

/// Build the broker binary and spawn it on `port`, streaming its output to
/// `broker_log`. The caller owns the returned child and must tear it down.
///
/// # Errors
///
/// Returns a build failure, an I/O error creating the log or spawning the process.
pub(crate) fn spawn(repository_root: &Path, port: u16, broker_log: &Path) -> Result<Child, XtaskError> {
    local_broker::build(repository_root)?;
    let binary = repository_root.join("target/release/andromeda");
    let log = File::create(broker_log)?;
    let errors = log.try_clone()?;
    let child = Command::new(binary)
        .current_dir(repository_root)
        .arg("--port")
        .arg(port.to_string())
        .env("ANDROMEDA__DATABASE__URL", DATABASE_URL)
        .env("ANDROMEDA__TEMPORAL__DEFAULT_LAST_N", DEFAULT_LAST_N)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(errors))
        .spawn()?;
    Ok(child)
}

/// Poll the broker's health endpoint until it answers or the budget is spent,
/// failing fast if the process exits during startup.
///
/// # Errors
///
/// Returns [`XtaskError::BrokerStartupExit`] if the broker exits first,
/// [`XtaskError::BrokerNotReady`] if the budget is spent, and
/// [`XtaskError::HealthClient`] if the probe client cannot be built.
pub(crate) fn await_ready(child: &mut Child, port: u16) -> Result<(), XtaskError> {
    let client = Client::builder().timeout(Duration::from_secs(2)).build()?;
    let url = format!("http://localhost:{port}/ngsi-ld/v1/health");
    for _attempt in 0..READINESS_ATTEMPTS {
        if readiness::probe(&client, &url) {
            return Ok(());
        }
        if child.try_wait()?.is_some() {
            return Err(XtaskError::BrokerStartupExit);
        }
        sleep(Duration::from_secs(1));
    }
    Err(XtaskError::BrokerNotReady(port))
}
