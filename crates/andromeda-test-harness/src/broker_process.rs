//! A broker process a test starts for itself.
//!
//! Some cases need a broker the tier does not run: one configured with an
//! `[identity] advertised_url`, one that has never held a context in memory, or
//! one paging its collections under a pagination policy of its own. The broker
//! is the binary under test, started on free ports against the tier's database
//! with the tier's settings, and stopped when the value is dropped. The binary
//! is named by the test, which alone can read `CARGO_BIN_EXE_andromeda`.

use crate::configuration::{DATABASE_ENDPOINT_ENV, DEFAULT_DATABASE_ENDPOINT};
use andromeda_test_mocks::free_port::free_loopback_port;
use reqwest::Client;
use std::{
    env::{temp_dir, var},
    fs::{File, read_to_string},
    io::Error as IoError,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::Duration,
};
use thiserror::Error;
use tokio::time::{Instant, sleep};

/// How long a started broker is given to answer its health endpoint, or to
/// exit when it is expected to refuse its configuration.
const STARTUP_DEADLINE: Duration = Duration::from_secs(120);

/// The settings every base-tier broker runs with beside its ports and
/// database (`crates/andromeda-xtask/src/local_broker.rs`).
const TIER_SETTINGS: [(&str, &str); 10] = [
    ("ANDROMEDA__TEMPORAL__DEFAULT_LAST_N", "19"),
    ("ANDROMEDA__TEMPORAL__MAX_LAST_N", "19"),
    ("ANDROMEDA__TEMPORAL__EXPIRATION_CHECK_INTERVAL", "1s"),
    ("ANDROMEDA__JSONLD__HOSTED_CONTEXT_REVALIDATION", "2s"),
    ("ANDROMEDA__DATABASE__MAX_CONNECTIONS", "8"),
    ("ANDROMEDA__TENANCY__TENANT_MAX_CONNECTIONS", "2"),
    ("ANDROMEDA__TENANCY__RUNTIME_IDLE_TTL", "60s"),
    ("ANDROMEDA__TENANCY__RUNTIME_MAX_CACHED", "4"),
    ("ANDROMEDA__TENANCY__JANITOR_INTERVAL", "1s"),
    ("ANDROMEDA__EXTENSIONS__TENANT_MANAGEMENT", "enabled"),
];

/// A broker process that could not be started or observed as a test expects.
#[derive(Debug, Error)]
pub enum BrokerProcessError {
    /// No free loopback port could be found for the broker or its management
    /// plane.
    #[error("no free loopback port for the broker: {0}")]
    Port(#[source] IoError),
    /// The broker's log file could not be created, duplicated or read.
    #[error("the broker log {path} is unusable: {source}")]
    Log {
        /// The log file.
        path: PathBuf,
        /// What failed.
        #[source]
        source: IoError,
    },
    /// The binary could not be started.
    #[error("the broker binary could not be started: {0}")]
    Spawn(#[source] IoError),
    /// The running process could not be observed.
    #[error("the broker process could not be observed: {0}")]
    Observe(#[source] IoError),
    /// No HTTP client could be built to probe the broker's health.
    #[error("no health client: {0}")]
    HealthClient(#[source] reqwest::Error),
    /// The broker exited before it answered its health endpoint.
    #[error("the broker exited during startup; its log is {}", log.display())]
    ExitedDuringStartup {
        /// The broker's log file.
        log: PathBuf,
    },
    /// The broker did not answer its health endpoint within the deadline.
    #[error("the broker never answered; its log is {}", log.display())]
    NeverAnswered {
        /// The broker's log file.
        log: PathBuf,
    },
    /// The broker kept running when it was expected to refuse its
    /// configuration.
    #[error("the broker kept running; its log is {}", log.display())]
    KeptRunning {
        /// The broker's log file.
        log: PathBuf,
    },
}

/// A running broker process of the test's own.
pub struct BrokerProcess {
    child: Child,
    port: u16,
    log: PathBuf,
}

/// A broker process that exited during startup instead of serving.
pub struct RefusedStart {
    /// How the process exited.
    pub status: ExitStatus,
    /// What the process wrote to its standard output and error.
    pub log: String,
}

impl BrokerProcess {
    /// Start `binary` with the tier's settings and `extra`, and wait until it
    /// answers its health endpoint.
    ///
    /// # Errors
    ///
    /// Returns the [`BrokerProcessError`] of starting the process, or
    /// [`BrokerProcessError::ExitedDuringStartup`] and
    /// [`BrokerProcessError::NeverAnswered`] when it does not come up.
    pub async fn start(binary: &Path, extra: &[(&str, &str)]) -> Result<BrokerProcess, BrokerProcessError> {
        let mut broker = BrokerProcess::spawn(binary, extra)?;
        broker.await_ready().await?;
        Ok(broker)
    }

    /// Start `binary` with the tier's settings and `extra`, expecting it to
    /// refuse its configuration and exit, and report how it exited.
    ///
    /// # Errors
    ///
    /// Returns the [`BrokerProcessError`] of starting or observing the
    /// process, and [`BrokerProcessError::KeptRunning`] when it is still
    /// running at the startup deadline.
    pub async fn refused_start(binary: &Path, extra: &[(&str, &str)]) -> Result<RefusedStart, BrokerProcessError> {
        let mut broker = BrokerProcess::spawn(binary, extra)?;
        let deadline = Instant::now() + STARTUP_DEADLINE;
        loop {
            if let Some(status) = broker.child.try_wait().map_err(BrokerProcessError::Observe)? {
                let log = read_to_string(&broker.log).map_err(|source| BrokerProcessError::Log {
                    path: broker.log.clone(), // clone: the error names the file the broker keeps
                    source,
                })?;
                return Ok(RefusedStart { status, log });
            }
            if Instant::now() >= deadline {
                return Err(BrokerProcessError::KeptRunning { log: broker.log.clone() }); // clone: the error names the file the broker keeps
            }
            sleep(Duration::from_millis(100)).await;
        }
    }

    fn spawn(binary: &Path, extra: &[(&str, &str)]) -> Result<BrokerProcess, BrokerProcessError> {
        let port = free_loopback_port().map_err(BrokerProcessError::Port)?;
        let management_port = free_loopback_port().map_err(BrokerProcessError::Port)?;
        let log = temp_dir().join(format!("andromeda-test-broker-{port}.log"));
        let unusable_log = |source| BrokerProcessError::Log {
            path: log.clone(), // clone: the error names the file the broker keeps
            source,
        };
        let output = File::create(&log).map_err(unusable_log)?;
        let errors = output.try_clone().map_err(unusable_log)?;
        let database = var(DATABASE_ENDPOINT_ENV).unwrap_or_else(|_| DEFAULT_DATABASE_ENDPOINT.to_owned());
        let child = Command::new(binary)
            .env("ANDROMEDA__SERVER__HOST", "127.0.0.1")
            .env("ANDROMEDA__SERVER__PORT", port.to_string())
            .env("ANDROMEDA__MANAGEMENT__PORT", management_port.to_string())
            .env("ANDROMEDA__DATABASE__URL", database)
            .envs(TIER_SETTINGS)
            .envs(extra.iter().copied())
            .stdout(Stdio::from(output))
            .stderr(Stdio::from(errors))
            .spawn()
            .map_err(BrokerProcessError::Spawn)?;
        Ok(BrokerProcess { child, port, log })
    }

    async fn await_ready(&mut self) -> Result<(), BrokerProcessError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(BrokerProcessError::HealthClient)?;
        let deadline = Instant::now() + STARTUP_DEADLINE;
        loop {
            let answered = client.get(self.url("health")).send().await.is_ok_and(|response| response.status().is_success());
            if answered {
                return Ok(());
            }
            if self.child.try_wait().map_err(BrokerProcessError::Observe)?.is_some() {
                return Err(BrokerProcessError::ExitedDuringStartup { log: self.log.clone() }); // clone: the error names the file the broker keeps
            }
            if Instant::now() >= deadline {
                return Err(BrokerProcessError::NeverAnswered { log: self.log.clone() }); // clone: the error names the file the broker keeps
            }
            sleep(Duration::from_millis(200)).await;
        }
    }

    /// The absolute URL of `path` on this broker's NGSI-LD API.
    #[must_use]
    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}/ngsi-ld/v1/{path}", self.port)
    }
}

impl Drop for BrokerProcess {
    fn drop(&mut self) {
        // A broker that already exited has nothing to kill; either way the
        // process is reaped so it leaves no zombie behind.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
