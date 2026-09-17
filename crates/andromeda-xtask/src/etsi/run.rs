//! The end-to-end ETSI run: provision the pinned suite, bootstrap its robot
//! virtualenv, bring up the dev compose stack on a wiped database, build and spawn
//! the broker, apply the patch sets, run robot, and optionally open the report.

use crate::{
    cli,
    compose::{self, ImageBuild},
    error::XtaskError,
    etsi::{
        self,
        broker,
        patch::{self, Material},
        robot,
        suite,
        venv,
    },
};
use clap::Args;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Child,
};

/// The dev compose stack backing an ETSI run: PostgreSQL and the MQTT brokers.
const COMPOSE_FILE: &str = "docker/compose/dev/docker-compose.yaml";
/// The robot artifacts cleared from the output directory before a run.
const ROBOT_ARTIFACTS: [&str; 4] = ["output.xml", "log.html", "report.html", "errors.log"];

/// Arguments to `xtask etsi run`.
#[derive(Args)]
pub(crate) struct RunArgs {
    /// Directory receiving robot's `output.xml`, `log.html`, `report.html`, and logs.
    #[arg(long, value_name = "DIR")]
    output: PathBuf,
    /// Judge the broker against the material exactly as the suite ships it, skipping
    /// the corrected-material patch set.
    #[arg(long)]
    as_shipped: bool,
    /// Broker listen port.
    #[arg(long, default_value_t = 1056)]
    port: u16,
    /// Seconds any one test may run before robot times it out.
    #[arg(long, default_value_t = 300)]
    timeout: u16,
    /// Open `report.html` in the default browser once the run finishes.
    #[arg(long)]
    open: bool,
    /// Leave the compose stack and broker running after the run.
    #[arg(long)]
    keep: bool,
    /// An argument forwarded to robot, before the resolved targets; repeatable.
    #[arg(long = "robot-arg", value_name = "ARG")]
    robot_args: Vec<String>,
    /// Suite subtrees to run; defaults to the whole suite.
    #[arg(value_name = "SUBTREE")]
    subtrees: Vec<String>,
}

impl RunArgs {
    /// Provision the suite and run it against a freshly built broker.
    ///
    /// # Errors
    ///
    /// Propagates any provisioning, virtualenv, compose, build, readiness, patch, or
    /// robot-spawn failure.
    pub(crate) fn execute(self) -> Result<(), XtaskError> {
        let repository_root = cli::repository_root();
        let output = prepare_output_dir(&self.output)?;

        suite::provision()?;
        suite::configure(self.port)?;
        venv::ensure()?;
        let targets = robot::resolve_targets(&etsi::checkout_dir(), &self.subtrees)?;

        let compose_file = repository_root.join(COMPOSE_FILE);
        compose::down(&repository_root, &compose_file, None);
        let mut teardown = RunTeardown {
            repository_root: &repository_root,
            compose_file,
            keep: self.keep,
            broker: None,
        };
        compose::up(&repository_root, &teardown.compose_file, None, ImageBuild::Build)?;

        let mut broker = broker::spawn(&repository_root, self.port, &output.join("broker.log"))?;
        broker::await_ready(&mut broker, self.port)?;
        teardown.broker = Some(broker);

        // Held to the end of the run: dropping the guard reverts the patches while
        // the checkout is still intact, before the stack is torn down.
        let _patches = patch::PatchGuard::apply(self.material())?;

        let code = robot::run(&output, self.timeout, &self.robot_args, &targets)?;
        println!("robot exited {code}; results in {}", output.display());

        if self.open {
            open_report(&output);
        }
        Ok(())
    }

    /// Which material the run judges the broker against.
    const fn material(&self) -> Material {
        if self.as_shipped { Material::AsShipped } else { Material::Corrected }
    }
}

/// Tears down an ETSI run: kills the spawned broker, then brings the dev compose
/// stack down, unless the run asked to keep them.
struct RunTeardown<'root> {
    repository_root: &'root Path,
    compose_file: PathBuf,
    keep: bool,
    broker: Option<Child>,
}

impl Drop for RunTeardown<'_> {
    fn drop(&mut self) {
        if let Some(broker) = &mut self.broker {
            // Best-effort: a broker that already exited cannot be killed or waited on.
            let _killed = broker.kill();
            let _waited = broker.wait();
        }
        if !self.keep {
            compose::down(self.repository_root, &self.compose_file, None);
        }
    }
}

/// Create the output directory if absent, resolve it to an absolute path so robot's
/// checkout-relative working directory cannot confuse it, and clear prior artifacts.
fn prepare_output_dir(output: &Path) -> Result<PathBuf, XtaskError> {
    fs::create_dir_all(output)?;
    let output = fs::canonicalize(output)?;
    for artifact in ROBOT_ARTIFACTS {
        let path = output.join(artifact);
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    Ok(output)
}

/// Open `report.html` in the default browser, warning rather than failing when it
/// is absent or the platform has no opener.
fn open_report(output: &Path) {
    let report = output.join("report.html");
    if !report.is_file() {
        eprintln!("xtask: no report.html to open at {}", report.display());
        return;
    }
    if let Err(error) = open::that(&report) {
        eprintln!("xtask: could not open {}: {error}", report.display());
    }
}
