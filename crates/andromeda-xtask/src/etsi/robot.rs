//! The robot invocation: resolving requested suite subtrees against the checkout
//! and assembling the command line. The suite's own `ErrorListener` is deliberately
//! not loaded -- its end-of-suite diff is quadratic in the accumulated error log
//! and reads as a hung run, while adding nothing `output.xml` does not already hold.

use crate::{
    error::XtaskError,
    etsi::{self, venv},
};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// The suite's own root under the checkout; a bare subtree resolves beneath it.
const SUITE_ROOT: &str = "TP/NGSI-LD";
/// Robot's console width, matching the recorded run record.
const CONSOLE_WIDTH: &str = "150";
/// The vendored `HttpCtrl` fork the distributed cases are written against, as a
/// checkout-relative import root. The suite ships this fork under `libraries/`
/// (its own `pyproject.toml` excludes the directory from packaging) with a stub
/// matcher that tolerates the query parameters a broker appends when it forwards
/// (`local`, `limit`, and the like); the `requirements.txt` pin installs stock
/// `robotframework-httpctrl` instead, whose matcher is an exact URL comparison and
/// so never matches a forwarded request. Prepending this to robot's import path
/// shadows the installed package for the run, so `Set Stub Reply` answers the
/// broker's forwarded request rather than blocking until the broker times out.
const HTTPCTRL_FORK_PYTHONPATH: &str = "libraries/robotframework-httpctrl/src";

/// Which topology a run targets, selecting the mutually exclusive `dist-ops` tag
/// filter so the single-broker run and the distributed run never share a case.
/// The `dist-ops` test-level tag is carried by every `DistributedOperations` case
/// and nowhere else in the suite, so the two scopes partition it exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TopologyScope {
    /// The default run: the whole suite minus the distributed-operations tree.
    SingleBroker,
    /// Only the distributed-operations tree, driven against the suite's in-process
    /// mock context source.
    Distributed,
}

impl TopologyScope {
    /// The robot tag filter selecting this scope's cases: `--exclude dist-ops` for
    /// the single-broker run, `--include dist-ops` for the distributed run.
    const fn tag_filter(self) -> [&'static str; 2] {
        match self {
            TopologyScope::SingleBroker => ["--exclude", "dist-ops"],
            TopologyScope::Distributed => ["--include", "dist-ops"],
        }
    }

    /// The output subdirectory this scope's pass writes into, keeping the two passes'
    /// robot artifacts (`output.xml` and its siblings) from overwriting each other
    /// when both run under one output directory.
    pub(crate) const fn output_subdir(self) -> &'static str {
        match self {
            TopologyScope::SingleBroker => "single-broker",
            TopologyScope::Distributed => "distributed",
        }
    }

    /// Whether this scope isolates each suite onto its own materialised tenant by
    /// default. The single-broker tree isolates -- a shared tenant makes every
    /// list/aggregate/discovery case depend on suite order and leftover state --
    /// while the distributed tree keeps the shared tenant its mock topology and the
    /// authoritative four-broker tier were validated against.
    pub(crate) const fn isolates_tenants_by_default(self) -> bool {
        matches!(self, TopologyScope::SingleBroker)
    }

    /// The checkout-relative import root prepended to robot's Python path for this
    /// scope, or `None` when the scope needs none. Only the distributed run needs
    /// the vendored `HttpCtrl` fork; the single-broker run leaves the installed
    /// package in place, since its `HttpCtrl`-using cases never exercise the
    /// broker-forwarded stub matcher.
    const fn library_pythonpath(self) -> Option<&'static str> {
        match self {
            TopologyScope::SingleBroker => None,
            TopologyScope::Distributed => Some(HTTPCTRL_FORK_PYTHONPATH),
        }
    }
}

/// Resolve the requested subtrees to checkout-relative paths, defaulting to the
/// whole suite when none are given.
///
/// # Errors
///
/// Returns [`XtaskError::UnknownSubtree`] for a subtree that exists nowhere.
pub(crate) fn resolve_targets(checkout: &Path, requested: &[String]) -> Result<Vec<String>, XtaskError> {
    if requested.is_empty() {
        return Ok(vec![SUITE_ROOT.to_owned()]);
    }
    requested.iter().map(|subtree| resolve_subtree(checkout, subtree)).collect()
}

/// Resolve one requested subtree, accepting a path relative to the checkout root,
/// one relative to the suite root, or a literal existing path.
fn resolve_subtree(checkout: &Path, requested: &str) -> Result<String, XtaskError> {
    let under_root = format!("{SUITE_ROOT}/{requested}");
    if checkout.join(requested).exists() {
        Ok(requested.to_owned())
    } else if checkout.join(&under_root).exists() {
        Ok(under_root)
    } else if Path::new(requested).exists() {
        Ok(requested.to_owned())
    } else {
        Err(XtaskError::UnknownSubtree(PathBuf::from(requested)))
    }
}

/// Run robot against the resolved targets from within the checkout, returning its
/// exit code. A non-zero code means some test cases failed, which is a normal
/// outcome the caller reports rather than an error; only a failure to spawn robot
/// is an error.
///
/// # Errors
///
/// Returns an I/O error when robot cannot be spawned.
pub(crate) fn run(
    output_dir: &Path,
    timeout_seconds: u16,
    isolate_tenants: bool,
    scope: TopologyScope,
    extra: &[String],
    targets: &[String],
) -> Result<i32, XtaskError> {
    let plugins = etsi::resource_dir().join("robot-plugins");
    let modifier = plugins.join("EtsiTestTimeout.py");
    let listener = isolate_tenants.then(|| plugins.join("TenantIsolation.py"));
    let args = robot_args(output_dir, timeout_seconds, &modifier, listener.as_deref(), scope, extra, targets);
    let status = Command::new(venv::robot_binary()).current_dir(etsi::checkout_dir()).args(&args).status()?;
    Ok(status.code().unwrap_or(1))
}

/// Assemble robot's arguments: output directory, console width, the per-test
/// timeout pre-run modifier, the per-suite tenant-isolation listener when one is
/// requested, the topology scope's `HttpCtrl` import root when it needs one, the
/// scope's `dist-ops` tag filter, any caller-supplied arguments, then the targets.
fn robot_args(
    output_dir: &Path,
    timeout_seconds: u16,
    modifier: &Path,
    listener: Option<&Path>,
    scope: TopologyScope,
    extra: &[String],
    targets: &[String],
) -> Vec<String> {
    let mut args = vec![
        "--outputdir".to_owned(),
        output_dir.display().to_string(),
        "-W".to_owned(),
        CONSOLE_WIDTH.to_owned(),
        "--prerunmodifier".to_owned(),
        format!("{}:{timeout_seconds}", modifier.display()),
    ];
    if let Some(listener) = listener {
        args.push("--listener".to_owned());
        args.push(listener.display().to_string());
    }
    if let Some(pythonpath) = scope.library_pythonpath() {
        args.push("--pythonpath".to_owned());
        args.push(pythonpath.to_owned());
    }
    args.extend(scope.tag_filter().into_iter().map(str::to_owned));
    args.extend(extra.iter().cloned());
    args.extend(targets.iter().cloned());
    args
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::etsi::robot::{SUITE_ROOT, TopologyScope, resolve_subtree, resolve_targets, robot_args};
    use std::{
        env,
        fs,
        path::{Path, PathBuf},
        process,
        thread,
    };

    #[test]
    fn no_requested_subtrees_default_to_the_whole_suite() {
        let targets = resolve_targets(Path::new("/nonexistent"), &[]).expect("the default needs no filesystem");
        assert_eq!(targets, vec![SUITE_ROOT.to_owned()]);
    }

    #[test]
    fn a_bare_subtree_resolves_beneath_the_suite_root() {
        let temporary = tempdir();
        fs::create_dir_all(temporary.join(SUITE_ROOT).join("001_02")).expect("create the suite subtree");
        let resolved = resolve_subtree(&temporary, "001_02").expect("the subtree resolves under the suite root");
        assert_eq!(resolved, format!("{SUITE_ROOT}/001_02"));
        fs::remove_dir_all(&temporary).expect("clean up the temporary tree");
    }

    #[test]
    fn a_checkout_relative_subtree_resolves_verbatim() {
        let temporary = tempdir();
        fs::create_dir_all(temporary.join("resources")).expect("create a checkout-relative path");
        let resolved = resolve_subtree(&temporary, "resources").expect("the path resolves verbatim");
        assert_eq!(resolved, "resources");
        fs::remove_dir_all(&temporary).expect("clean up the temporary tree");
    }

    #[test]
    fn an_unknown_subtree_is_rejected() {
        let temporary = tempdir();
        fs::create_dir_all(&temporary).expect("create the empty checkout");
        assert!(resolve_subtree(&temporary, "does/not/exist").is_err());
        fs::remove_dir_all(&temporary).expect("clean up the temporary tree");
    }

    #[test]
    fn robot_arguments_are_assembled_in_order() {
        let args = robot_args(
            Path::new("/out"),
            300,
            Path::new("/res/EtsiTestTimeout.py"),
            None,
            TopologyScope::SingleBroker,
            &["--suite".to_owned(), "Foo".to_owned()],
            &["TP/NGSI-LD".to_owned()],
        );
        assert_eq!(
            args,
            vec![
                "--outputdir".to_owned(),
                "/out".to_owned(),
                "-W".to_owned(),
                "150".to_owned(),
                "--prerunmodifier".to_owned(),
                "/res/EtsiTestTimeout.py:300".to_owned(),
                "--exclude".to_owned(),
                "dist-ops".to_owned(),
                "--suite".to_owned(),
                "Foo".to_owned(),
                "TP/NGSI-LD".to_owned(),
            ]
        );
    }

    #[test]
    fn isolate_tenants_inserts_the_listener_after_the_modifier() {
        let args = robot_args(
            Path::new("/out"),
            300,
            Path::new("/res/EtsiTestTimeout.py"),
            Some(Path::new("/res/TenantIsolation.py")),
            TopologyScope::SingleBroker,
            &[],
            &["TP/NGSI-LD".to_owned()],
        );
        assert_eq!(
            args,
            vec![
                "--outputdir".to_owned(),
                "/out".to_owned(),
                "-W".to_owned(),
                "150".to_owned(),
                "--prerunmodifier".to_owned(),
                "/res/EtsiTestTimeout.py:300".to_owned(),
                "--listener".to_owned(),
                "/res/TenantIsolation.py".to_owned(),
                "--exclude".to_owned(),
                "dist-ops".to_owned(),
                "TP/NGSI-LD".to_owned(),
            ]
        );
    }

    #[test]
    fn the_single_broker_scope_excludes_the_distributed_tag_with_no_import_override() {
        let single = robot_args(
            Path::new("/out"),
            300,
            Path::new("/res/EtsiTestTimeout.py"),
            None,
            TopologyScope::SingleBroker,
            &[],
            &["TP/NGSI-LD".to_owned()],
        );
        assert!(
            !single.iter().any(|arg| arg == "--pythonpath"),
            "the single-broker run leaves the installed HttpCtrl in place"
        );
        assert_eq!(
            &single[6..8],
            &["--exclude".to_owned(), "dist-ops".to_owned()],
            "the single-broker run excludes the distributed tree",
        );
    }

    #[test]
    fn the_distributed_scope_prepends_the_httpctrl_fork_then_includes_the_distributed_tag() {
        let distributed = robot_args(
            Path::new("/out"),
            300,
            Path::new("/res/EtsiTestTimeout.py"),
            None,
            TopologyScope::Distributed,
            &[],
            &["TP/NGSI-LD".to_owned()],
        );
        assert_eq!(
            &distributed[6..10],
            &[
                "--pythonpath".to_owned(),
                "libraries/robotframework-httpctrl/src".to_owned(),
                "--include".to_owned(),
                "dist-ops".to_owned(),
            ],
            "the distributed run loads the vendored HttpCtrl fork, then runs only the distributed tree",
        );
    }

    /// A unique temporary directory path under the system temp root for one test.
    fn tempdir() -> PathBuf {
        let unique = format!("andromeda-xtask-robot-{}-{:?}", process::id(), thread::current().id());
        env::temp_dir().join(unique)
    }
}
