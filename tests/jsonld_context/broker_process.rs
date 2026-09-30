//! A broker process a test starts for itself.
//!
//! Two cases need a broker the tier does not run: one configured with an
//! `[identity] advertised_url` (clause 6.2's apiRoot with a prefix), and one that
//! has never held a context in memory, to prove that what a response names
//! does not depend on which cache tier happens to hold a document. The broker
//! is the binary under test, started on free ports against the tier's database
//! with the tier's settings, and stopped when the value is dropped.

use andromeda_test_harness::configuration::{DATABASE_ENDPOINT_ENV, DEFAULT_DATABASE_ENDPOINT};
use andromeda_test_mocks::free_port::free_loopback_port;
use reqwest::Client;
use std::{
    env::{temp_dir, var},
    fs::File,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::time::{Instant, sleep};

/// How long a started broker is given to answer its health endpoint.
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

/// A running broker process of the test's own.
pub struct BrokerProcess {
    child: Child,
    port: u16,
    log: PathBuf,
}

impl BrokerProcess {
    /// Start a broker with the tier's settings and `extra`, and wait until it
    /// answers its health endpoint.
    pub async fn start(extra: &[(&str, &str)]) -> BrokerProcess {
        let port = free_loopback_port().expect("a free port for the broker");
        let management_port = free_loopback_port().expect("a free port for its management plane");
        let log = temp_dir().join(format!("andromeda-jsonld-context-broker-{port}.log"));
        let output = File::create(&log).expect("the broker log file is writable");
        let errors = output.try_clone().expect("the broker log file is duplicable");
        let database = var(DATABASE_ENDPOINT_ENV).unwrap_or_else(|_| DEFAULT_DATABASE_ENDPOINT.to_owned());
        let child = Command::new(env!("CARGO_BIN_EXE_andromeda"))
            .env("ANDROMEDA__SERVER__HOST", "127.0.0.1")
            .env("ANDROMEDA__SERVER__PORT", port.to_string())
            .env("ANDROMEDA__MANAGEMENT__PORT", management_port.to_string())
            .env("ANDROMEDA__DATABASE__URL", database)
            .envs(TIER_SETTINGS)
            .envs(extra.iter().copied())
            .stdout(Stdio::from(output))
            .stderr(Stdio::from(errors))
            .spawn()
            .expect("the broker binary starts");
        let mut broker = BrokerProcess { child, port, log };
        broker.await_ready().await;
        broker
    }

    async fn await_ready(&mut self) {
        let client = Client::builder().timeout(Duration::from_secs(2)).build().expect("a health client");
        let deadline = Instant::now() + STARTUP_DEADLINE;
        loop {
            let answered = client.get(self.url("health")).send().await.is_ok_and(|response| response.status().is_success());
            if answered {
                return;
            }
            let exited = self.child.try_wait().expect("the broker process is observable");
            assert!(exited.is_none(), "the broker exited during startup; its log is {}", self.log.display());
            assert!(Instant::now() < deadline, "the broker never answered; its log is {}", self.log.display());
            sleep(Duration::from_millis(200)).await;
        }
    }

    /// The absolute URL of `path` on this broker's NGSI-LD API.
    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}/ngsi-ld/v1/{path}", self.port)
    }
}

impl Drop for BrokerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
