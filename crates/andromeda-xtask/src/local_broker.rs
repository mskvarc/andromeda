//! The host-broker dev loop for the base tier: an incremental cargo build and two
//! spawned broker processes, in place of the containerized `broker` profile.

use crate::{
    error::XtaskError,
    readiness::{self, Readiness},
};
use reqwest::blocking::Client;
use std::{
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};

/// Database endpoint both host brokers share.
const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";
/// The tier pins `default_last_n` to 19 so the `SYSTEM_LAST_N_CAP` regressions hit the 206 cut.
const DEFAULT_LAST_N: &str = "19";
/// The tier pins `max_last_n` to the same 19, so a `lastN` above it is paged
/// backwards as the base-tier regressions expect.
const MAX_LAST_N: &str = "19";
/// The settings every base-tier broker runs with beside its ports, identical to
/// the containerized `broker` profile in `docker/compose/test/base/docker-compose.yaml`,
/// which documents why each one is needed: a 1s expiry sweep for the expiration
/// suite, pool bounds that keep two brokers under PostgreSQL's connection limit,
/// and the tenant-management extension the tenant suites and cleanup rely on.
const TIER_SETTINGS: [(&str, &str); 7] = [
    ("ANDROMEDA__TEMPORAL__EXPIRATION_CHECK_INTERVAL", "1s"),
    ("ANDROMEDA__DATABASE__MAX_CONNECTIONS", "16"),
    ("ANDROMEDA__TENANCY__TENANT_MAX_CONNECTIONS", "4"),
    ("ANDROMEDA__TENANCY__RUNTIME_IDLE_TTL", "60s"),
    ("ANDROMEDA__TENANCY__RUNTIME_MAX_CACHED", "4"),
    ("ANDROMEDA__TENANCY__JANITOR_INTERVAL", "1s"),
    ("ANDROMEDA__EXTENSIONS__TENANT_MANAGEMENT", "enabled"),
];
/// Health-poll budget: 120 attempts one second apart.
const READINESS_ATTEMPTS: u16 = 120;

/// One of the two host brokers the base tier runs.
///
/// Each runs as its own process on the host, so beside its data-plane port it
/// needs its own management-plane port: the brokers' shared default would let
/// only one of them bind it.
#[derive(Clone, Copy)]
pub(crate) enum HostBroker {
    /// The broker under test; forwarding headers are not trusted.
    UnderTest,
    /// The trusted-edge broker; forwarding headers are trusted.
    TrustedEdge,
}

impl HostBroker {
    /// Both host brokers, in spawn order.
    pub(crate) const ALL: [HostBroker; 2] = [HostBroker::UnderTest, HostBroker::TrustedEdge];

    /// The data-plane port the suites address.
    const fn port(self) -> u16 {
        match self {
            HostBroker::UnderTest => 1056,
            HostBroker::TrustedEdge => 1057,
        }
    }

    /// The management-plane port.
    const fn management_port(self) -> u16 {
        match self {
            HostBroker::UnderTest => 1086,
            HostBroker::TrustedEdge => 1087,
        }
    }
}

/// Fail fast if any host-broker data-plane or management-plane port is already bound.
///
/// # Errors
///
/// Returns [`XtaskError::PortInUse`] for the first port that cannot be bound.
pub(crate) fn preflight_ports() -> Result<(), XtaskError> {
    for port in HostBroker::ALL.into_iter().flat_map(|broker| [broker.port(), broker.management_port()]) {
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
pub(crate) fn spawn_broker(repository_root: &Path, broker: HostBroker) -> Result<Child, XtaskError> {
    let binary = repository_root.join("target/release/andromeda");
    let mut command = Command::new(binary);
    command
        .current_dir(repository_root)
        .env("ANDROMEDA__SERVER__HOST", "127.0.0.1")
        .env("ANDROMEDA__SERVER__PORT", broker.port().to_string())
        .env("ANDROMEDA__MANAGEMENT__PORT", broker.management_port().to_string())
        .env("ANDROMEDA__DATABASE__URL", DATABASE_URL)
        .env("ANDROMEDA__TEMPORAL__DEFAULT_LAST_N", DEFAULT_LAST_N)
        .env("ANDROMEDA__TEMPORAL__MAX_LAST_N", MAX_LAST_N)
        .envs(TIER_SETTINGS)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match broker {
        HostBroker::UnderTest => {}
        HostBroker::TrustedEdge => {
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
    for port in HostBroker::ALL.map(HostBroker::port) {
        let url = format!("http://127.0.0.1:{port}/ngsi-ld/v1/health");
        if readiness::await_health(&client, &url, READINESS_ATTEMPTS) == Readiness::Unready {
            return Err(XtaskError::BrokerNotReady(port));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::local_broker::HostBroker;
    use std::collections::HashSet;

    /// Both host brokers run in one network namespace, so every data-plane and
    /// management-plane port they bind must be distinct.
    #[test]
    fn the_host_brokers_bind_four_distinct_ports() {
        let ports: HashSet<u16> = HostBroker::ALL
            .into_iter()
            .flat_map(|broker| [broker.port(), broker.management_port()])
            .collect();
        assert_eq!(ports.len(), 4, "{ports:?}");
    }
}
