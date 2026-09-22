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
        merge,
        patch::{self, Material},
        robot::{self, TopologyScope},
        suite,
        venv,
    },
};
use clap::{Args, ValueEnum};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Child,
};

/// The dev compose stack backing an ETSI run: PostgreSQL and the MQTT brokers.
const COMPOSE_FILE: &str = "docker/compose/dev/docker-compose.yaml";
/// The robot artifacts cleared from the output directory before a run.
const ROBOT_ARTIFACTS: [&str; 4] = ["output.xml", "log.html", "report.html", "errors.log"];
/// The broker's listen port. This must equal the `localhost:1056` the infra patch
/// (`suite-delta/infra` i01 `variables.py`) bakes into the checkout: the harness no
/// longer rewrites the suite's configuration at runtime, so the two are only kept in
/// step by both naming the same fixed port.
const BROKER_PORT: u16 = 1056;

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
    /// Return a non-zero exit when any test case fails, turning the run from
    /// report-only into a hard gate. Without it the run reports failures and still
    /// succeeds; the `etsi` CI job passes it (scoped to `--scope base`) so a broker
    /// regression against the single-broker tree turns the job red.
    #[arg(long)]
    strict: bool,
    /// Seconds any one test may run before robot times it out.
    #[arg(long, default_value_t = 300)]
    timeout: u16,
    /// What becomes of the report and the backing services once the run ends.
    #[command(flatten)]
    aftermath: RunAftermath,
    /// How suites are assigned NGSI-LD tenants, from the two conflicting flags.
    #[command(flatten)]
    tenant_isolation: TenantIsolationArgs,
    /// Which topology passes to drive against the one spawned broker; defaults to the
    /// whole suite (both passes).
    #[arg(long, value_enum, default_value_t = EtsiScope::All)]
    scope: EtsiScope,
    /// An argument forwarded to robot, before the resolved targets; repeatable.
    #[arg(long = "robot-arg", value_name = "ARG")]
    robot_args: Vec<String>,
    /// Suite subtrees to run; defaults to the whole suite.
    #[arg(value_name = "SUBTREE")]
    subtrees: Vec<String>,
}

/// Which topology passes an `xtask etsi run` drives against the single spawned
/// broker. The suite partitions on the `dist-ops` tag, so each pass runs a disjoint
/// slice of cases; a run drives one slice or both, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum EtsiScope {
    /// The single-broker tree only: the whole suite minus the distributed tree.
    Base,
    /// The distributed-operations tree only, against the in-process mock context source.
    Distop,
    /// Both passes in order: the single-broker tree, then the distributed tree.
    All,
}

impl EtsiScope {
    /// The ordered topology passes this scope drives. `All` runs the single-broker
    /// tree then the distributed tree; `Base` and `Distop` run one pass each.
    fn passes(self) -> Vec<TopologyScope> {
        match self {
            EtsiScope::Base => vec![TopologyScope::SingleBroker],
            EtsiScope::Distop => vec![TopologyScope::Distributed],
            EtsiScope::All => vec![TopologyScope::SingleBroker, TopologyScope::Distributed],
        }
    }
}

/// What becomes of the run's artifacts and backing services once it finishes.
#[derive(Args)]
struct RunAftermath {
    /// Open `report.html` in the default browser once the run finishes.
    #[arg(long)]
    open: bool,
    /// Leave the compose stack and broker running after the run.
    #[arg(long)]
    keep: bool,
}

/// The tenant-isolation choice, expressed as two conflicting flags so that
/// neither one set leaves the decision to the topology default.
#[derive(Args)]
struct TenantIsolationArgs {
    /// Force each suite onto its own materialised NGSI-LD tenant for every pass that
    /// runs, overriding the topology-derived default. Isolation is already the
    /// single-broker default, so this only changes the distributed pass.
    #[arg(long, conflicts_with = "shared_tenant")]
    isolate_tenants: bool,
    /// Run every suite against one shared NGSI-LD tenant, reproducing the raw
    /// suite's cross-suite state accumulation. Isolation is on by default for
    /// the single-broker tree because a shared tenant makes every case that
    /// lists or aggregates across a tenant depend on suite execution order and
    /// leftover state; pass this only to observe that unisolated behaviour
    /// deliberately.
    #[arg(long, conflicts_with = "isolate_tenants")]
    shared_tenant: bool,
}

impl TenantIsolationArgs {
    /// The forced isolation choice, or `None` when neither flag is set and the
    /// topology default stands.
    const fn forced(&self) -> Option<bool> {
        if self.isolate_tenants {
            Some(true)
        } else if self.shared_tenant {
            Some(false)
        } else {
            None
        }
    }
}

impl RunArgs {
    /// Provision the suite and run it against a freshly built broker.
    ///
    /// # Errors
    ///
    /// Propagates any provisioning, virtualenv, compose, build, readiness, patch,
    /// robot-spawn, or merge failure, and under `--strict`
    /// [`XtaskError::EtsiTestsFailed`] when any test case failed.
    pub(crate) fn execute(self) -> Result<(), XtaskError> {
        let repository_root = cli::repository_root();
        let output = prepare_output_dir(&self.output)?;

        suite::provision()?;
        venv::ensure()?;
        let targets = robot::resolve_targets(&etsi::checkout_dir(), &self.subtrees)?;

        let compose_file = repository_root.join(COMPOSE_FILE);
        compose::down(&repository_root, &compose_file, None);
        let mut teardown = RunTeardown {
            repository_root: &repository_root,
            compose_file,
            keep: self.aftermath.keep,
            broker: None,
        };
        compose::up(&repository_root, &teardown.compose_file, None, ImageBuild::Build)?;

        let mut broker = broker::spawn(&repository_root, BROKER_PORT, &output.join("broker.log"))?;
        broker::await_ready(&mut broker, BROKER_PORT)?;
        teardown.broker = Some(broker);

        // Held to the end of the run: dropping the guard reverts the patches while
        // the checkout is still intact, before the stack is torn down. The corrected
        // material covers the whole suite, so both passes judge the broker against
        // this one patched checkout.
        let _patches = patch::PatchGuard::apply(self.material())?;

        // Each pass writes into its own subdirectory so the two robot runs never
        // overwrite each other's output.xml; the merged report is written across
        // them afterwards.
        let forced = self.tenant_isolation.forced();
        let mut pass_outputs = Vec::new();
        let mut total_failed: usize = 0;
        for scope in self.scope.passes() {
            let pass_output = prepare_pass_dir(&output, scope)?;
            let code = robot::run(
                &pass_output,
                self.timeout,
                resolve_isolate_tenants(forced, scope),
                scope,
                &self.robot_args,
                &targets,
            )?;
            println!("robot [{}] exited {code}; results in {}", scope.output_subdir(), pass_output.display());
            // robot's exit code is the number of failed cases (capped at 250), so any
            // positive code is a failing pass; accumulate it for the strict gate.
            total_failed = total_failed.saturating_add(usize::try_from(code).unwrap_or(0));
            pass_outputs.push(pass_output.join("output.xml"));
        }

        let merged = pass_outputs.len() > 1;
        if merged {
            merge::merge(&output, &pass_outputs)?;
            println!("merged report in {}", output.display());
        }

        if self.aftermath.open {
            // The unified report sits at the top of the output directory once passes
            // are merged; a single-pass run's report stays in its one subdirectory.
            let report_dir: &Path = if merged {
                output.as_path()
            } else {
                pass_outputs.first().and_then(|output_xml| output_xml.parent()).unwrap_or(output.as_path())
            };
            open_report(report_dir);
        }

        // Report-only by default: failing cases are reported and the run still
        // succeeds, so only an infrastructure error is a failure. `--strict` turns a
        // failing case into a failing run for the CI gate.
        if self.strict && total_failed > 0 {
            return Err(XtaskError::EtsiTestsFailed(total_failed));
        }
        Ok(())
    }

    /// Which material the run judges the broker against.
    const fn material(&self) -> Material {
        if self.as_shipped { Material::AsShipped } else { Material::Corrected }
    }
}

/// Whether a pass of `scope` isolates each suite onto its own materialised tenant:
/// the forced choice when either conflicting flag is set (`--isolate-tenants` forces
/// it on, `--shared-tenant` off), otherwise the scope's topology default.
const fn resolve_isolate_tenants(forced: Option<bool>, scope: TopologyScope) -> bool {
    match forced {
        Some(forced) => forced,
        None => scope.isolates_tenants_by_default(),
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
/// checkout-relative working directory cannot confuse it, and clear prior artifacts
/// (including any merged report from an earlier run).
fn prepare_output_dir(output: &Path) -> Result<PathBuf, XtaskError> {
    fs::create_dir_all(output)?;
    let output = fs::canonicalize(output)?;
    clear_artifacts(&output)?;
    Ok(output)
}

/// Create and clear one pass's subdirectory under the run's output directory, so a
/// prior run's robot artifacts for this pass never bleed into the current one.
fn prepare_pass_dir(output: &Path, scope: TopologyScope) -> Result<PathBuf, XtaskError> {
    let pass_output = output.join(scope.output_subdir());
    fs::create_dir_all(&pass_output)?;
    clear_artifacts(&pass_output)?;
    Ok(pass_output)
}

/// Remove any robot artifacts left in `dir` by an earlier run.
fn clear_artifacts(dir: &Path) -> Result<(), XtaskError> {
    for artifact in ROBOT_ARTIFACTS {
        let path = dir.join(artifact);
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use crate::etsi::{
        robot::TopologyScope,
        run::{EtsiScope, TenantIsolationArgs, resolve_isolate_tenants},
    };
    use clap::ValueEnum;

    #[test]
    fn base_scope_runs_the_single_broker_pass_only() {
        assert_eq!(EtsiScope::Base.passes(), vec![TopologyScope::SingleBroker]);
    }

    #[test]
    fn distop_scope_runs_the_distributed_pass_only() {
        assert_eq!(EtsiScope::Distop.passes(), vec![TopologyScope::Distributed]);
    }

    #[test]
    fn all_scope_runs_the_single_broker_pass_then_the_distributed_pass() {
        assert_eq!(EtsiScope::All.passes(), vec![TopologyScope::SingleBroker, TopologyScope::Distributed]);
    }

    #[test]
    fn each_scope_parses_from_its_kebab_case_value() {
        for (value, expected) in [("base", EtsiScope::Base), ("distop", EtsiScope::Distop), ("all", EtsiScope::All)] {
            let parsed = EtsiScope::from_str(value, false).expect("the kebab-case value parses");
            assert_eq!(parsed, expected);
            let name = expected.to_possible_value().expect("every variant has a value").get_name().to_owned();
            assert_eq!(name, value, "the value round-trips to its kebab-case spelling");
        }
    }

    #[test]
    fn with_no_override_each_pass_takes_its_topology_default() {
        // The single-broker tree isolates; the distributed tree keeps the shared tenant.
        assert!(resolve_isolate_tenants(None, TopologyScope::SingleBroker));
        assert!(!resolve_isolate_tenants(None, TopologyScope::Distributed));
    }

    #[test]
    fn shared_tenant_forces_isolation_off_for_both_passes() {
        let forced = TenantIsolationArgs {
            isolate_tenants: false,
            shared_tenant: true,
        }
        .forced();
        assert_eq!(forced, Some(false));
        assert!(!resolve_isolate_tenants(forced, TopologyScope::SingleBroker));
        assert!(!resolve_isolate_tenants(forced, TopologyScope::Distributed));
    }

    #[test]
    fn isolate_tenants_forces_isolation_on_for_both_passes() {
        let forced = TenantIsolationArgs {
            isolate_tenants: true,
            shared_tenant: false,
        }
        .forced();
        assert_eq!(forced, Some(true));
        assert!(resolve_isolate_tenants(forced, TopologyScope::SingleBroker));
        assert!(resolve_isolate_tenants(forced, TopologyScope::Distributed));
    }

    #[test]
    fn neither_flag_leaves_the_choice_to_the_topology_default() {
        let forced = TenantIsolationArgs {
            isolate_tenants: false,
            shared_tenant: false,
        }
        .forced();
        assert_eq!(forced, None);
    }
}
