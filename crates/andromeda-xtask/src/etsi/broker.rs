//! The broker under test for an ETSI run: built in release mode and spawned as a
//! single host process on the run's port, against the dev compose database, with
//! the temporal page size the suite's pagination precondition requires.

use crate::{error::XtaskError, etsi, local_broker, readiness};
use iri_rs::{Iri, iri};
use reqwest::blocking::Client;
use serde_json::{Value, json};
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
/// Per-tenant pool size for the run. Robot drives the suite serially, so one
/// request touches one tenant at a time and a pool of two is ample; the low cap
/// keeps a run that cycles through a fresh tenant per suite (`--isolate-tenants`)
/// well within PostgreSQL's connection limit.
const TENANT_MAX_CONNECTIONS: &str = "2";
/// How many tenant runtimes stay cached at once. With the small pool above this
/// bounds the live connection count (roughly cache times pool), and the janitor
/// evicts the least-recently-used runtimes past it.
const RUNTIME_MAX_CACHED: &str = "8";
/// How long an idle tenant runtime is held before the janitor closes its pool.
/// Kept at thirty seconds so a tenant that only holds a periodic subscription --
/// a `timeInterval` notification fires with no intervening request -- is not
/// evicted before its timer fires (case `046_02` uses `timeInterval: 10`); a
/// shorter retention would silence that notification. The connection pressure is
/// bounded by the runtime-cache cap (enforced synchronously on cold insert), not
/// by this idle window.
const RUNTIME_IDLE_TTL_SECS: &str = "30";
/// How often the janitor sweeps. The runtime-cache cap is now enforced
/// synchronously on cold insert (the trim runs as each fresh tenant's runtime is
/// built, not only on the sweep), so the open PostgreSQL connection count is
/// bounded per request rather than left to climb under `--isolate-tenants` churn.
/// This cadence therefore governs only how responsively idle and orphaned
/// runtimes are reclaimed; five seconds is ample for that under the suite's
/// serial drive.
const JANITOR_INTERVAL_SECS: &str = "5";
/// The compound `@context` the suite's fixtures and queries reference, and the
/// suite `@context` the compound pulls in by a relative reference. Both are
/// served from the checkout so the broker never fetches them from forge: under
/// `--isolate-tenants` every fresh tenant would otherwise cold-fetch these same
/// URLs (the remote-context cache is per-tenant), and the resulting burst of
/// identical requests draws the occasional dropped connection that surfaces as a
/// spurious `LdContextNotAvailable` (504) on an entity create.
const SUITE_COMPOUND_CONTEXT_URL: Iri<&str> =
    iri!("https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite-compound.jsonld");
/// The suite `@context` the compound references relatively; the loader resolves
/// the relative reference against the compound's URL to exactly this URL.
const SUITE_CONTEXT_URL: Iri<&str> =
    iri!("https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite.jsonld");
/// The checkout-relative directory holding both suite context documents.
const SUITE_CONTEXT_DIR: &str = "resources/jsonld-contexts";

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
        .env("ANDROMEDA__TENANCY__TENANT_MAX_CONNECTIONS", TENANT_MAX_CONNECTIONS)
        .env("ANDROMEDA__TENANCY__RUNTIME_MAX_CACHED", RUNTIME_MAX_CACHED)
        .env("ANDROMEDA__TENANCY__RUNTIME_IDLE_TTL_SECS", RUNTIME_IDLE_TTL_SECS)
        .env("ANDROMEDA__TENANCY__JANITOR_INTERVAL_SECS", JANITOR_INTERVAL_SECS)
        .env("ANDROMEDA__JSONLD__PRELOAD_CONTEXTS", suite_context_preload())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(errors))
        .spawn()?;
    Ok(child)
}

/// The JSON `url -> file` map that pins the suite's two `@context` documents to
/// their copies in the checkout, for `ANDROMEDA__JSONLD__PRELOAD_CONTEXTS`. The
/// broker registers these offline and never fetches them, so a run cannot fail
/// on forge being briefly unreachable. `serde_json` renders the paths, so a
/// checkout path containing a quote or backslash is escaped rather than
/// producing malformed JSON.
fn suite_context_preload() -> String {
    let contexts = etsi::checkout_dir().join(SUITE_CONTEXT_DIR);
    let compound = contexts.join("ngsi-ld-test-suite-compound.jsonld");
    let suite = contexts.join("ngsi-ld-test-suite.jsonld");
    let compound_url = SUITE_COMPOUND_CONTEXT_URL.as_str();
    let suite_url = SUITE_CONTEXT_URL.as_str();
    let map = json!({
        compound_url: Value::from(compound.to_string_lossy().into_owned()),
        suite_url: Value::from(suite.to_string_lossy().into_owned()),
    });
    map.to_string()
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
